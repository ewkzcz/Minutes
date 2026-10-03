//! 前端可调用的命令（Tauri invoke）。

use crate::asr::{self, AsrParams};
use crate::config::{Settings, DEFAULT_ASR_MODELS, DEFAULT_LLM_MODELS};
use crate::db::Db;
use crate::llm::Llm;
use crate::session::{self, Engine};
use crate::{audio, export};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::State;

pub struct App {
    pub engine: Arc<Engine>,
    pub db: Arc<Db>,
}

type R<T> = Result<T, String>;

fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

// ------------------------------------------------------------ 设置

#[tauri::command]
pub fn get_settings(app: State<App>) -> Value {
    let s = Settings::load(&app.db);
    let custom: Vec<String> = serde_json::from_str(&app.db.setting("custom_models").unwrap_or("[]".into())).unwrap_or_default();
    let mut llm: Vec<String> = DEFAULT_LLM_MODELS.iter().map(|s| s.to_string()).collect();
    llm.extend(custom.into_iter().filter(|m| !DEFAULT_LLM_MODELS.contains(&m.as_str())));
    json!({"settings": s, "asr_models": DEFAULT_ASR_MODELS, "llm_models": llm, "devices": audio::list_devices(), "active_session": app.engine.active_id()})
}

#[tauri::command]
pub fn save_settings(app: State<App>, values: HashMap<String, String>) -> R<()> {
    const ALLOWED: &[&str] = &[
        "api_key", "workspace_id", "llm_base_url", "asr_model", "asr_lang", "summary_model", "correct_model", "correct_enabled",
        "correct_strength", "correct_timeout_ms", "interval_min", "mic_device", "audio_source", "theme", "custom_models",
    ];
    for (k, v) in values {
        if ALLOWED.contains(&k.as_str()) {
            app.db.set_setting(&k, &v).map_err(e)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn test_connection(app: State<'_, App>) -> R<Value> {
    let s = Settings::load(&app.db);
    if s.api_key.is_empty() {
        return Err("未配置百炼密钥".into());
    }
    let llm = Llm::new(s.llm_url(), s.api_key.clone());
    let llm_res = llm.chat(&s.summary_model, "你是测试助手", "回复 ok", 0.0, 8, Duration::from_secs(20)).await;
    let asr_res = asr::probe(&AsrParams { url: s.asr_url(), api_key: s.api_key.clone(), model: s.asr_model.clone(), lang: s.asr_lang.clone() }).await;
    Ok(json!({
        "llm": {"ok": llm_res.is_ok(), "model": s.summary_model, "error": llm_res.err().map(|x| x.to_string())},
        "asr": {"ok": asr_res.is_ok(), "model": s.asr_model, "error": asr_res.err().map(|x| x.to_string())}
    }))
}

// ------------------------------------------------------------ 录制

#[tauri::command]
pub async fn start_session(app: State<'_, App>, title: String, template_id: Option<i64>, audio_file: Option<String>) -> R<Value> {
    app.engine.start(title, template_id, audio_file).await.map_err(e)
}

#[tauri::command]
pub fn pause_session(app: State<App>, paused: bool) {
    app.engine.pause(paused)
}

#[tauri::command]
pub fn stop_session(app: State<App>) {
    app.engine.stop()
}

#[tauri::command]
pub fn summarize_now(app: State<App>) {
    app.engine.summarize_now()
}

#[tauri::command]
pub fn set_live_model(app: State<App>, model: String) {
    app.engine.set_model(&model)
}

// ------------------------------------------------------------ 历史

#[tauri::command]
pub fn sessions_list(app: State<App>, query: String, only_fav: bool) -> R<Vec<Value>> {
    let like = format!("%{}%", query.trim().replace('%', "\\%").replace('_', "\\_"));
    let q = query.trim().is_empty();
    let fav = only_fav as i64;
    app.db
        .query(
            "SELECT s.id,s.title,s.started_at,s.duration_ms,s.status,s.pinned,s.favorite,s.template_name,
               (SELECT COUNT(*) FROM segments g WHERE g.session_id=s.id) AS seg_count,
               (SELECT g.text FROM segments g WHERE g.session_id=s.id AND (g.text LIKE ?2 ESCAPE '\\' OR g.raw LIKE ?2 ESCAPE '\\') ORDER BY g.seq LIMIT 1) AS hit,
               substr(s.report,1,120) AS report_head
             FROM sessions s
             WHERE (?1=1 OR s.title LIKE ?2 ESCAPE '\\' OR s.report LIKE ?2 ESCAPE '\\'
                    OR EXISTS(SELECT 1 FROM segments g WHERE g.session_id=s.id AND (g.text LIKE ?2 ESCAPE '\\' OR g.raw LIKE ?2 ESCAPE '\\'))
                    OR EXISTS(SELECT 1 FROM stages t WHERE t.session_id=s.id AND t.content LIKE ?2 ESCAPE '\\'))
               AND (?3=0 OR s.favorite=1)
             ORDER BY s.pinned DESC, s.started_at DESC",
            &[&(q as i64), &like, &fav],
        )
        .map_err(e)
}

#[tauri::command]
pub fn session_get(app: State<App>, id: i64) -> R<Value> {
    let s = app.db.one("SELECT id,title,started_at,ended_at,duration_ms,status,pinned,favorite,report,template_name,template_snapshot,asr_model,llm_model FROM sessions WHERE id=?", &[&id]).map_err(e)?.ok_or("会议不存在")?;
    let segs = app.db.query("SELECT id,seq,begin_ms,end_ms,raw,text,stage FROM segments WHERE session_id=? ORDER BY seq", &[&id]).map_err(e)?;
    let stages = app.db.query("SELECT id,idx,from_ms,to_ms,content,model FROM stages WHERE session_id=? ORDER BY idx", &[&id]).map_err(e)?;
    Ok(json!({"session": s, "segments": segs, "stages": stages}))
}

#[tauri::command]
pub fn session_update(app: State<App>, id: i64, title: Option<String>, pinned: Option<bool>, favorite: Option<bool>) -> R<()> {
    if let Some(t) = title {
        app.db.exec("UPDATE sessions SET title=? WHERE id=?", &[&t, &id]).map_err(e)?;
    }
    if let Some(p) = pinned {
        app.db.exec("UPDATE sessions SET pinned=? WHERE id=?", &[&(p as i64), &id]).map_err(e)?;
    }
    if let Some(f) = favorite {
        app.db.exec("UPDATE sessions SET favorite=? WHERE id=?", &[&(f as i64), &id]).map_err(e)?;
    }
    Ok(())
}

#[tauri::command]
pub fn sessions_delete(app: State<App>, ids: Vec<i64>) -> R<usize> {
    let active = app.engine.active_id();
    let mut n = 0;
    for id in ids {
        if Some(id) == active {
            continue;
        }
        app.db.exec("DELETE FROM segments WHERE session_id=?", &[&id]).map_err(e)?;
        app.db.exec("DELETE FROM stages WHERE session_id=?", &[&id]).map_err(e)?;
        n += app.db.exec("DELETE FROM sessions WHERE id=?", &[&id]).map_err(e)?;
    }
    Ok(n)
}

#[tauri::command]
pub async fn regenerate_report(app: State<'_, App>, id: i64, model: Option<String>) -> R<String> {
    let s = Settings::load(&app.db);
    let llm = Llm::new(s.llm_url(), s.api_key.clone());
    let model = model.unwrap_or_else(|| s.summary_model.clone());
    session::generate_report(&app.db, &llm, &s, id, &model).await.map_err(e)
}

#[tauri::command]
pub fn session_export(app: State<App>, id: i64, opts: Value, path: String) -> R<String> {
    let text = export::build(&app.db, id, &opts).map_err(e)?;
    std::fs::write(&path, &text).map_err(e)?;
    Ok(path)
}

#[tauri::command]
pub fn session_markdown(app: State<App>, id: i64, opts: Value) -> R<String> {
    export::build(&app.db, id, &opts).map_err(e)
}

// ------------------------------------------------------------ 词库

#[tauri::command]
pub fn lexicon_list(app: State<App>) -> R<Vec<Value>> {
    app.db.query("SELECT id,term,misspellings,weight,enabled,note FROM lexicon ORDER BY id DESC", &[]).map_err(e)
}

#[tauri::command]
pub fn lexicon_save(app: State<App>, id: Option<i64>, term: String, misspellings: String, weight: String, note: String) -> R<i64> {
    let term = term.trim().to_string();
    if term.is_empty() {
        return Err("专有词不能为空".into());
    }
    match id {
        Some(id) => {
            app.db.exec("UPDATE lexicon SET term=?,misspellings=?,weight=?,note=? WHERE id=?", &[&term, &misspellings, &weight, &note, &id]).map_err(e)?;
            Ok(id)
        }
        None => app.db.insert("INSERT INTO lexicon(term,misspellings,weight,note) VALUES(?,?,?,?)", &[&term, &misspellings, &weight, &note]).map_err(e),
    }
}

#[tauri::command]
pub fn lexicon_toggle(app: State<App>, id: i64, enabled: bool) -> R<()> {
    app.db.exec("UPDATE lexicon SET enabled=? WHERE id=?", &[&(enabled as i64), &id]).map_err(e)?;
    Ok(())
}

#[tauri::command]
pub fn lexicon_delete(app: State<App>, ids: Vec<i64>) -> R<()> {
    for id in ids {
        app.db.exec("DELETE FROM lexicon WHERE id=?", &[&id]).map_err(e)?;
    }
    Ok(())
}

#[tauri::command]
pub fn lexicon_test(app: State<App>, text: String) -> String {
    let m = crate::correct::Matcher::new(session::load_terms(&app.db));
    m.correct(&text, &Settings::load(&app.db).correct_strength)
}

// ------------------------------------------------------------ 提示词模板

#[tauri::command]
pub fn templates_list(app: State<App>) -> R<Vec<Value>> {
    app.db.query("SELECT id,name,task,output_req,filters,interval_min,builtin FROM templates ORDER BY id", &[]).map_err(e)
}

#[tauri::command]
pub fn template_save(app: State<App>, id: Option<i64>, name: String, task: String, output_req: String, filters: String, interval_min: i64) -> R<i64> {
    if name.trim().is_empty() {
        return Err("模板名称不能为空".into());
    }
    let interval = interval_min.clamp(1, 60);
    match id {
        Some(id) => {
            app.db.exec("UPDATE templates SET name=?,task=?,output_req=?,filters=?,interval_min=? WHERE id=?", &[&name, &task, &output_req, &filters, &interval, &id]).map_err(e)?;
            Ok(id)
        }
        None => app.db.insert("INSERT INTO templates(name,task,output_req,filters,interval_min) VALUES(?,?,?,?,?)", &[&name, &task, &output_req, &filters, &interval]).map_err(e),
    }
}

#[tauri::command]
pub fn template_delete(app: State<App>, id: i64) -> R<()> {
    let n = app.db.one("SELECT COUNT(*) AS n FROM templates", &[]).map_err(e)?.map(|v| v["n"].as_i64().unwrap_or(0)).unwrap_or(0);
    if n <= 1 {
        return Err("至少保留一个模板".into());
    }
    app.db.exec("DELETE FROM templates WHERE id=?", &[&id]).map_err(e)?;
    Ok(())
}

#[tauri::command]
pub fn templates_restore(app: State<App>) -> R<usize> {
    app.db.restore_builtins().map_err(e)
}
