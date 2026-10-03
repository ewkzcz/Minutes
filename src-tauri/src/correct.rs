//! 两级实时纠错：
//! 第一级 词库快速纠错（误识别写法精确替换 + 拼音相似度），纯内存，partial 与 final 都用；
//! 第二级 上下文纠错（低延迟对话模型，超时或改动过大即放弃），仅 final 使用。

use crate::llm::Llm;
use pinyin::ToPinyin;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Term {
    pub term: String,
    pub misspellings: Vec<String>,
    pub weight: String,
}

struct Pattern {
    joined: String,
    term_idx: usize,
}

pub struct Matcher {
    pub terms: Vec<Term>,
    exact: Vec<(String, usize)>,
    fuzzy: Vec<(usize, Vec<Pattern>)>, // (音节数, 模式)
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

fn syllables(chars: &[char]) -> Option<Vec<String>> {
    let s: String = chars.iter().collect();
    let out: Vec<String> = s.as_str().to_pinyin().flatten().map(|p| p.plain().to_string()).collect();
    (out.len() == chars.len()).then_some(out)
}

fn threshold(weight: &str, strength: &str) -> f64 {
    let base: f64 = match weight {
        "high" => 0.80,
        "low" => 0.92,
        _ => 0.86,
    };
    let off = match strength {
        "conservative" => 0.05,
        "aggressive" => -0.05,
        _ => 0.0,
    };
    (base + off).min(0.99)
}

impl Matcher {
    pub fn new(terms: Vec<Term>) -> Self {
        let mut exact: Vec<(String, usize)> = vec![];
        let mut fuzzy: Vec<(usize, Vec<Pattern>)> = vec![];
        for (i, t) in terms.iter().enumerate() {
            for m in &t.misspellings {
                if !m.is_empty() && *m != t.term {
                    exact.push((m.clone(), i));
                }
            }
            let mut sources: Vec<&String> = t.misspellings.iter().collect();
            if t.term.chars().all(is_cjk) {
                sources.push(&t.term);
            }
            for src in sources {
                let chars: Vec<char> = src.chars().collect();
                if chars.len() < 2 || !chars.iter().all(|c| is_cjk(*c)) {
                    continue;
                }
                if let Some(sy) = syllables(&chars) {
                    let pat = Pattern { joined: sy.concat(), term_idx: i };
                    match fuzzy.iter_mut().find(|(n, _)| *n == sy.len()) {
                        Some((_, v)) => v.push(pat),
                        None => fuzzy.push((sy.len(), vec![pat])),
                    }
                }
            }
        }
        exact.sort_by_key(|(m, _)| std::cmp::Reverse(m.chars().count()));
        Matcher { terms, exact, fuzzy }
    }

    /// 返回纠正后的文本；未命中返回原文。
    pub fn correct(&self, text: &str, strength: &str) -> String {
        if self.terms.is_empty() || text.is_empty() {
            return text.to_string();
        }
        let mut cur = text.to_string();
        // 英文专有词：忽略大小写，统一为词库写法（如 kafka → Kafka）
        for t in &self.terms {
            if t.term.is_ascii() && t.term.len() >= 2 {
                cur = replace_ascii_ci(&cur, &t.term);
            }
        }
        for (wrong, i) in &self.exact {
            if cur.contains(wrong.as_str()) {
                cur = cur.replace(wrong.as_str(), &self.terms[*i].term);
            }
        }
        if self.fuzzy.is_empty() {
            return cur;
        }
        let chars: Vec<char> = cur.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if !is_cjk(chars[i]) {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            let mut j = i;
            while j < chars.len() && is_cjk(chars[j]) {
                j += 1;
            }
            out.push_str(&self.fuzzy_run(&chars[i..j], strength));
            i = j;
        }
        out
    }

    fn fuzzy_run(&self, run: &[char], strength: &str) -> String {
        let Some(sy) = syllables(run) else { return run.iter().collect() };
        let mut out = String::new();
        let mut i = 0;
        while i < run.len() {
            let mut best: Option<(f64, usize, usize)> = None; // (分数, 窗口长, 词条)
            for (size, pats) in &self.fuzzy {
                if i + size > run.len() {
                    continue;
                }
                let window: String = run[i..i + size].iter().collect();
                let window_py = sy[i..i + size].concat();
                for p in pats {
                    let t = &self.terms[p.term_idx];
                    if window == t.term {
                        continue;
                    }
                    let score = strsim::normalized_levenshtein(&window_py, &p.joined);
                    if score >= threshold(&t.weight, strength) && best.map_or(true, |b| score > b.0) {
                        best = Some((score, *size, p.term_idx));
                    }
                }
            }
            match best {
                Some((_, size, idx)) => {
                    out.push_str(&self.terms[idx].term);
                    i += size;
                }
                None => {
                    out.push(run[i]);
                    i += 1;
                }
            }
        }
        out
    }
}

/// 忽略大小写替换整词（两侧不是英文字母 / 数字）为规范写法。
fn replace_ascii_ci(text: &str, term: &str) -> String {
    let lower = text.to_ascii_lowercase(); // ASCII 小写不改变字节长度
    let needle = term.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut last = 0;
    let mut from = 0;
    while let Some(pos) = lower[from..].find(&needle) {
        let start = from + pos;
        let end = start + needle.len();
        let before_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let after_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if before_ok && after_ok {
            out.push_str(&text[last..start]);
            out.push_str(term);
            last = end;
        }
        from = end;
    }
    out.push_str(&text[last..]);
    out
}

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

/// 第二级：结合最近对话与词表让模型纠错；超时 / 失败 / 改动过大均返回 None。
pub async fn context_correct(
    llm: &Llm,
    model: &str,
    strength: &str,
    terms: &[Term],
    history: &[String],
    sentence: &str,
    timeout_ms: u64,
) -> Option<String> {
    if sentence.chars().count() < 4 {
        return None;
    }
    let terms_txt: Vec<&str> = terms.iter().take(60).map(|t| t.term.as_str()).collect();
    let system = format!(
        "你是语音识别纠错器。根据对话上下文与专有词表，纠正用户最新一句识别文本中的错误。{}只输出纠正后的句子，不要解释，不要加引号。",
        strength_rule(strength)
    );
    let hist: Vec<&String> = history.iter().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect();
    let user = format!(
        "专有词表：{}\n最近内容：\n{}\n\n待纠正：{}",
        if terms_txt.is_empty() { "无".into() } else { terms_txt.join("、") },
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
    let ratio = 1.0 - strsim::normalized_levenshtein(sentence, &cand);
    (ratio <= max_edit_ratio(strength)).then_some(cand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexicon_exact_and_fuzzy() {
        let m = Matcher::new(vec![
            Term { term: "Kafka".into(), misspellings: vec!["卡夫卡".into()], weight: "mid".into() },
            Term { term: "灰度发布".into(), misspellings: vec![], weight: "high".into() },
        ]);
        assert_eq!(m.correct("我们用卡夫卡做队列", "balanced"), "我们用Kafka做队列");
        assert_eq!(m.correct("上线前要先辉度发布", "balanced"), "上线前要先灰度发布");
        assert_eq!(m.correct("没有专有名词", "balanced"), "没有专有名词");
        assert_eq!(m.correct("用的是kafka和KAFKA，不是kafkaesque", "balanced"), "用的是Kafka和Kafka，不是kafkaesque");
    }
}
