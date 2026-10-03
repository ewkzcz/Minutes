//! 上下文纠错：把最近对话与专有词表一并交给低延迟对话模型，由模型判断识别错误并改正；
//! 超时、失败或改动过大即放弃，仅 final 句使用。专有词表只作为纠错依据，不做任何机械替换。

use crate::llm::Llm;
use std::time::Duration;

/// 词表分隔符：换行、中英文逗号、顿号、中英文分号、制表符。
pub fn split_terms(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for t in text.split(|c| matches!(c, '\n' | '\r' | ',' | '，' | '、' | ';' | '；' | '\t')).map(str::trim) {
        if !t.is_empty() && !out.iter().any(|x| x.eq_ignore_ascii_case(t)) {
            out.push(t.to_string());
        }
    }
    out
}

/// 词表放进提示词的上限（字数），避免拖慢纠错。
const TERMS_BUDGET: usize = 3000;

fn strength_rule(strength: &str) -> &'static str {
    match strength {
        "conservative" => "只改正明显的同音字、错别字和专有名词，不确定时保持原样。",
        "aggressive" => "结合上下文改正识别错误，补全明显漏识别的虚词，但不得改变原意。",
        _ => "改正同音字、错别字、专有名词和明显的断句错误，不改变原意。",
    }
}

fn max_edit_ratio(strength: &str) -> f64 {
    match strength {
        "conservative" => 0.15,
        "aggressive" => 0.35,
        _ => 0.25,
    }
}

/// 结合最近对话与专有词表让模型纠错；超时 / 失败 / 改动过大均返回 None。
pub async fn context_correct(
    llm: &Llm,
    model: &str,
    strength: &str,
    terms: &[String],
    history: &[String],
    sentence: &str,
    timeout_ms: u64,
) -> Option<String> {
    if sentence.chars().count() < 4 {
        return None;
    }
    let mut used = 0;
    let terms: Vec<&str> = terms
        .iter()
        .take_while(|t| {
            used += t.chars().count() + 1;
            used <= TERMS_BUDGET
        })
        .map(String::as_str)
        .collect();
    let system = format!(
        "你是语音识别纠错器。依据两样东西纠正用户最新一句识别文本中的错误：一是最近对话的上下文，二是专有词表。\
专有词表是本场会议可能出现的人名、产品名、术语，语音识别常把它们识别成读音相近的其他字词或英文的音译（如把 Kafka 识别成“卡夫卡”）；\
当句中某处结合上下文明显指的是词表里的词时，改成词表中的写法，读音或语义对不上时不要硬套。{}只输出纠正后的句子，不要解释，不要加引号。",
        strength_rule(strength)
    );
    let hist: Vec<&String> = history.iter().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect();
    let user = format!(
        "专有词表：{}\n最近内容：\n{}\n\n待纠正：{}",
        if terms.is_empty() { "无".into() } else { terms.join("、") },
        if hist.is_empty() { "无".into() } else { hist.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n") },
        sentence
    );
    let max_tokens = (sentence.chars().count() as u32 * 2).max(64);
    let ans = tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        llm.chat(model, &system, &user, 0.0, max_tokens, Duration::from_millis(timeout_ms + 500)),
    )
    .await
    .ok()?
    .ok()?;
    let cand = ans.trim().trim_matches(|c| "「」\"'".contains(c)).to_string();
    if cand.is_empty() || cand == sentence {
        return None;
    }
    (edit_ratio(sentence, &cand, &terms) <= max_edit_ratio(strength)).then_some(cand)
}

/// 改动幅度：编辑距离 / 原句字数；改成词表写法的部分不计入（如“卡夫卡”→“Kafka”本身就是大改动）。
fn edit_ratio(sentence: &str, cand: &str, terms: &[&str]) -> f64 {
    let dist = strsim::levenshtein(sentence, cand);
    let credit: usize = terms
        .iter()
        .map(|t| cand.matches(t).count().saturating_sub(sentence.matches(t).count()) * t.chars().count())
        .sum();
    dist.saturating_sub(credit) as f64 / sentence.chars().count().max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_terms_all_separators() {
        assert_eq!(
            split_terms("Kafka\n灰度发布, 张三丰，Apache Flink、kafka；Redis;\t\n  "),
            vec!["Kafka", "灰度发布", "张三丰", "Apache Flink", "Redis"]
        );
    }

    #[test]
    fn term_rewrites_not_counted_as_edits() {
        let terms = ["Kafka"];
        assert_eq!(edit_ratio("我们用卡夫卡做队列", "我们用Kafka做队列", &terms), 0.0);
        assert!(edit_ratio("我们用卡夫卡做队列", "他们用卡夫卡排队", &terms) > 0.25);
    }
}
