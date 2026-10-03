//! 会议导出：总结 / 阶段纪要 / 纠错后转写 / ASR 原始转写 / 纠错对照。

use crate::db::Db;
use crate::session::{fmt_datetime, fmt_ms};
use anyhow::{anyhow, Result};
use serde_json::Value;

pub fn build(db: &Db, id: i64, opts: &Value) -> Result<String> {
    let flag = |k: &str| opts.get(k).and_then(|v| v.as_bool()).unwrap_or(true);
    let s = db.one("SELECT * FROM sessions WHERE id=?", &[&id])?.ok_or_else(|| anyhow!("会议不存在"))?;
    let segs = db.query("SELECT begin_ms,raw,text FROM segments WHERE session_id=? ORDER BY seq", &[&id])?;
    let stages = db.query("SELECT from_ms,to_ms,content FROM stages WHERE session_id=? ORDER BY idx", &[&id])?;
    let mut o = String::new();
    o.push_str(&format!("# {}\n\n", s["title"].as_str().unwrap_or("")));
    o.push_str(&format!(
        "> {} · 时长 {} · 模板：{}\n\n",
        fmt_datetime(s["started_at"].as_i64().unwrap_or(0)),
        fmt_ms(s["duration_ms"].as_i64().unwrap_or(0)),
        s["template_name"].as_str().unwrap_or("")
    ));
    if flag("report") && !s["report"].as_str().unwrap_or("").is_empty() {
        o.push_str(&format!("## 整体总结\n\n{}\n\n", s["report"].as_str().unwrap_or("")));
    }
    if flag("stages") && !stages.is_empty() {
        o.push_str("## 阶段纪要\n\n");
        for st in &stages {
            o.push_str(&format!("### {} – {}\n\n{}\n\n", fmt_ms(st["from_ms"].as_i64().unwrap_or(0)), fmt_ms(st["to_ms"].as_i64().unwrap_or(0)), st["content"].as_str().unwrap_or("")));
        }
    }
    if flag("text") {
        o.push_str("## 纠错后转写\n\n");
        for g in &segs {
            o.push_str(&format!("[{}] {}\n\n", fmt_ms(g["begin_ms"].as_i64().unwrap_or(0)), g["text"].as_str().unwrap_or("")));
        }
    }
    if flag("raw") {
        o.push_str("## ASR 原始转写\n\n");
        for g in &segs {
            o.push_str(&format!("[{}] {}\n\n", fmt_ms(g["begin_ms"].as_i64().unwrap_or(0)), g["raw"].as_str().unwrap_or("")));
        }
    }
    if flag("diff") {
        let changed: Vec<&Value> = segs.iter().filter(|g| g["raw"] != g["text"]).collect();
        o.push_str(&format!("## 纠错对照（共 {} 处）\n\n", changed.len()));
        for g in changed {
            o.push_str(&format!("- [{}] 原：{}\n  改：{}\n", fmt_ms(g["begin_ms"].as_i64().unwrap_or(0)), g["raw"].as_str().unwrap_or(""), g["text"].as_str().unwrap_or("")));
        }
    }
    if opts.get("plain").and_then(|v| v.as_bool()).unwrap_or(false) {
        o = o.lines().map(|l| l.trim_start_matches('#').trim_start().replace("**", "").replace("> ", "")).collect::<Vec<_>>().join("\n");
    }
    Ok(o)
}
