//! SQLite 存储：设置、会议、转写片段、阶段纪要、专有词库、提示词模板。

use anyhow::Result;
use rusqlite::{types::ValueRef, Connection, ToSql};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::sync::Mutex;

pub struct Db(Mutex<Connection>);

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS templates (
  id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, task TEXT NOT NULL DEFAULT '',
  output_req TEXT NOT NULL DEFAULT '', filters TEXT NOT NULL DEFAULT '[]',
  interval_min INTEGER NOT NULL DEFAULT 5, builtin INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS sessions (
  id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, template_id INTEGER,
  template_name TEXT NOT NULL DEFAULT '', template_snapshot TEXT NOT NULL DEFAULT '{}',
  started_at INTEGER NOT NULL, ended_at INTEGER, duration_ms INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'recording', pinned INTEGER NOT NULL DEFAULT 0,
  favorite INTEGER NOT NULL DEFAULT 0, report TEXT NOT NULL DEFAULT '',
  asr_model TEXT NOT NULL DEFAULT '', llm_model TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS segments (
  id INTEGER PRIMARY KEY AUTOINCREMENT, session_id INTEGER NOT NULL, seq INTEGER NOT NULL,
  begin_ms INTEGER NOT NULL DEFAULT 0, end_ms INTEGER NOT NULL DEFAULT 0,
  raw TEXT NOT NULL, text TEXT NOT NULL, stage TEXT NOT NULL DEFAULT 'raw'
);
CREATE INDEX IF NOT EXISTS idx_segments_session ON segments(session_id, seq);
CREATE TABLE IF NOT EXISTS stages (
  id INTEGER PRIMARY KEY AUTOINCREMENT, session_id INTEGER NOT NULL, idx INTEGER NOT NULL,
  from_ms INTEGER NOT NULL, to_ms INTEGER NOT NULL, content TEXT NOT NULL, model TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_stages_session ON stages(session_id, idx);
CREATE TABLE IF NOT EXISTS lexicon (
  id INTEGER PRIMARY KEY AUTOINCREMENT, term TEXT NOT NULL, misspellings TEXT NOT NULL DEFAULT '',
  weight TEXT NOT NULL DEFAULT 'mid', enabled INTEGER NOT NULL DEFAULT 1, note TEXT NOT NULL DEFAULT ''
);
"#;

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=OFF;")?;
        conn.execute_batch(SCHEMA)?;
        let db = Db(Mutex::new(conn));
        db.seed()?;
        db.merge_legacy_templates()?;
        Ok(db)
    }

    fn seed(&self) -> Result<()> {
        let n = self.one("SELECT COUNT(*) AS n FROM templates", &[])?.map(|v| v["n"].as_i64().unwrap_or(0)).unwrap_or(0);
        if n > 0 {
            return Ok(());
        }
        self.restore_builtins().map(|_| ())
    }

    /// 旧版模板把「输出要求」「筛选条件」分开存放，这里合并进唯一的系统提示词。
    fn merge_legacy_templates(&self) -> Result<()> {
        let rows = self.query("SELECT id,task,output_req,filters FROM templates WHERE output_req<>'' OR filters<>'[]'", &[])?;
        for r in rows {
            let mut task = r["task"].as_str().unwrap_or("").to_string();
            let req = r["output_req"].as_str().unwrap_or("").trim();
            if !req.is_empty() {
                task.push_str(&format!("\n\n最终输出要求：\n{req}"));
            }
            let filters: Vec<Value> = serde_json::from_str(r["filters"].as_str().unwrap_or("[]")).unwrap_or_default();
            let lines: Vec<String> = filters
                .iter()
                .filter(|f| !f["cond"].as_str().unwrap_or("").is_empty())
                .map(|f| format!("- {}（{}；{}）", f["cond"].as_str().unwrap_or(""), f["method"].as_str().unwrap_or(""), f["weight"].as_str().unwrap_or("")))
                .collect();
            if !lines.is_empty() {
                task.push_str(&format!("\n\n筛选条件（逐条给出「符合 / 不符合 / 未提及」）：\n{}", lines.join("\n")));
            }
            let id = r["id"].as_i64().unwrap_or(0);
            self.exec("UPDATE templates SET task=?, output_req='', filters='[]' WHERE id=?", &[&task, &id])?;
        }
        Ok(())
    }

    /// 补回缺失的预置模板（按名称判断，不覆盖用户已改的同名模板），返回新增个数。
    pub fn restore_builtins(&self) -> Result<usize> {
        let items: [(&str, &str, &str, Value); 3] = [
            (
                "面试复盘与评分",
                "你是一名资深技术面试记录员。会议为岗位面试，请持续记录候选人的项目经历、技术细节、量化成果与回答中的不足，并标注面试官尚未追问的点，便于面试后复盘。\n\n最终输出要求：\n1. 整体总结（不超过 200 字）\n2. 按维度打分（1–5）：技术深度 / 系统设计 / 沟通 / 稳定性，并附证据原话\n3. 条件筛选，逐条给出「符合 / 不符合 / 未提及」：3 年以上后端经验（必须）；有 Rust 或 Go 生产经验（加分）；能量化项目成果（加分）\n4. 待追问清单",
                "",
                json!([]),
            ),
            (
                "项目周会纪要",
                "你是项目周会记录员。请提炼各议题的讨论要点、达成的结论、待办事项（负责人与截止时间，未提及则标注「待定」）与风险。\n\n最终输出要求：\n1. 会议概要（不超过 150 字）\n2. 按议题列出结论\n3. 待办清单（表格：事项 / 负责人 / 时间）\n4. 风险与依赖",
                "",
                json!([]),
            ),
            (
                "通用会议纪要",
                "你是会议记录员。请客观提炼会议的细节、关键点与结论，不编造未出现的信息。\n\n最终输出要求：\n1. 总结\n2. 关键点\n3. 待办",
                "",
                json!([]),
            ),
        ];
        let mut added = 0;
        for (name, task, req, filters) in items {
            if self.one("SELECT id FROM templates WHERE name=?", &[&name])?.is_some() {
                continue;
            }
            self.exec(
                "INSERT INTO templates(name,task,output_req,filters,interval_min,builtin) VALUES(?,?,?,?,5,1)",
                &[&name, &task, &req, &filters.to_string()],
            )?;
            added += 1;
        }
        Ok(added)
    }

    pub fn exec(&self, sql: &str, params: &[&dyn ToSql]) -> Result<usize> {
        Ok(self.0.lock().unwrap().execute(sql, params)?)
    }

    pub fn insert(&self, sql: &str, params: &[&dyn ToSql]) -> Result<i64> {
        let conn = self.0.lock().unwrap();
        conn.execute(sql, params)?;
        Ok(conn.last_insert_rowid())
    }

    pub fn query(&self, sql: &str, params: &[&dyn ToSql]) -> Result<Vec<Value>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare(sql)?;
        let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
        let rows = stmt.query_map(params, |row| {
            let mut m = Map::new();
            for (i, name) in names.iter().enumerate() {
                let v = match row.get_ref(i)? {
                    ValueRef::Null | ValueRef::Blob(_) => Value::Null,
                    ValueRef::Integer(n) => json!(n),
                    ValueRef::Real(f) => json!(f),
                    ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
                };
                m.insert(name.clone(), v);
            }
            Ok(Value::Object(m))
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn one(&self, sql: &str, params: &[&dyn ToSql]) -> Result<Option<Value>> {
        Ok(self.query(sql, params)?.into_iter().next())
    }

    pub fn setting(&self, key: &str) -> Option<String> {
        self.one("SELECT value FROM settings WHERE key=?", &[&key]).ok().flatten().and_then(|v| v["value"].as_str().map(String::from))
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.exec("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=?2", &[&key, &value])?;
        Ok(())
    }
}
