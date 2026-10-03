//! 会议会话编排：麦克风 → 流式识别 → 实时纠错 → 每 N 分钟阶段整理 → 结束后生成最终总结。

use crate::asr::{self, AsrEvent, AsrParams, ConnEnd};
use crate::audio;
use crate::config::Settings;
use crate::correct::context_correct;
use crate::db::Db;
use crate::llm::Llm;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch, Mutex as AMutex};
use tokio::task::JoinHandle;

pub type Emitter = Arc<dyn Fn(&str, Value) + Send + Sync>;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

pub fn fmt_ms(ms: i64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

// ---------------------------------------------------------------- 模板 / 提示词

#[derive(Clone, Debug, Default)]
pub struct Template {
    pub name: String,
    pub task: String,
    pub output_req: String,
    pub filters: Vec<Value>,
    pub interval_min: u64,
}

impl Template {
    pub fn from_row(r: &Value) -> Self {
        Template {
            name: r["name"].as_str().unwrap_or("").into(),
            task: r["task"].as_str().unwrap_or("").into(),
            output_req: r["output_req"].as_str().unwrap_or("").into(),
            filters: serde_json::from_str(r["filters"].as_str().unwrap_or("[]")).unwrap_or_default(),
            interval_min: r["interval_min"].as_u64().unwrap_or(5).clamp(1, 60),
        }
    }

    fn filters_text(&self) -> String {
        self.filters
            .iter()
            .filter(|f| !f["cond"].as_str().unwrap_or("").is_empty())
            .map(|f| format!("- {}（判定：{}；{}）", f["cond"].as_str().unwrap_or(""), f["method"].as_str().unwrap_or("按转写内容判断"), f["weight"].as_str().unwrap_or("加分")))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn stage_system(&self, minutes: u64) -> String {
        let mut s = if self.task.is_empty() { "你是会议记录员。".to_string() } else { self.task.clone() };
        let f = self.filters_text();
        if !f.is_empty() {
            s.push_str(&format!("\n\n【需要关注的筛选条件】\n{f}"));
        }
        s.push_str(&format!(
            "\n\n现在做会议的阶段性整理（约每 {minutes} 分钟一次）。只依据给出的转写，用 Markdown 输出本段的：细节要点（关键词加粗）、关键点/结论/数字、需追问或待办（如有）。简洁，不复述，不编造；转写可能有识别错误，按上下文合理理解。"
        ));
        s
    }

    fn report_system(&self) -> String {
        let mut s = if self.task.is_empty() { "你是会议记录员。".to_string() } else { self.task.clone() };
        if !self.output_req.is_empty() {
            s.push_str(&format!("\n\n【最终输出要求】\n{}", self.output_req));
        }
        let f = self.filters_text();
        if !f.is_empty() {
            s.push_str(&format!("\n\n【筛选条件】\n{f}"));
        }
        s.push_str("\n\n请基于给出的阶段纪要与转写生成最终输出，使用 Markdown，严格按上面系统提示词里的输出要求，不编造转写中没有的信息。");
        s
    }
}

/// 本场会议使用的专有词：已启用词库中的已启用词条，词库须绑定了该模板或未绑定任何模板（通用）。
pub fn load_terms(db: &Db, template_id: i64) -> Vec<String> {
    let rows = db
        .query(
            "SELECT l.term FROM lexicon l JOIN lexicon_books b ON b.id=l.book_id \
             WHERE l.enabled=1 AND b.enabled=1 AND (NOT EXISTS (SELECT 1 FROM template_books t WHERE t.book_id=b.id) \
             OR EXISTS (SELECT 1 FROM template_books t WHERE t.book_id=b.id AND t.template_id=?)) ORDER BY b.id, l.id",
            &[&template_id],
        )
        .unwrap_or_default();
    let mut out: Vec<String> = vec![];
    for t in rows.iter().filter_map(|r| r["term"].as_str().map(str::trim)) {
        if !t.is_empty() && !out.iter().any(|x| x.eq_ignore_ascii_case(t)) {
            out.push(t.to_string());
        }
    }
    out
}

// ---------------------------------------------------------------- 阶段整理 / 最终总结

struct StageState {
    last_seg_id: i64,
    idx: i64,
}

fn lines(segs: &[Value]) -> String {
    segs.iter().map(|s| format!("[{}] {}", fmt_ms(s["begin_ms"].as_i64().unwrap_or(0)), s["text"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n")
}

/// 生成（或重新生成）会议最终总结并写库。
pub async fn generate_report(db: &Db, llm: &Llm, settings: &Settings, sid: i64, model: &str) -> Result<String> {
    let row = db.one("SELECT template_snapshot FROM sessions WHERE id=?", &[&sid])?.ok_or_else(|| anyhow!("会议不存在"))?;
    let snap: Value = serde_json::from_str(row["template_snapshot"].as_str().unwrap_or("{}")).unwrap_or(Value::Null);
    let t = Template::from_row(&snap);
    let stages = db.query("SELECT from_ms,to_ms,content FROM stages WHERE session_id=? ORDER BY idx", &[&sid])?;
    let segs = db.query("SELECT begin_ms,text FROM segments WHERE session_id=? ORDER BY seq", &[&sid])?;
    if segs.is_empty() {
        return Err(anyhow!("没有转写内容，无法生成总结"));
    }
    let transcript = lines(&segs);
    let mut user = String::new();
    if !stages.is_empty() {
        user.push_str("【阶段纪要】\n");
        for s in &stages {
            user.push_str(&format!("### {} – {}\n{}\n\n", fmt_ms(s["from_ms"].as_i64().unwrap_or(0)), fmt_ms(s["to_ms"].as_i64().unwrap_or(0)), s["content"].as_str().unwrap_or("")));
        }
    }
    let budget = if stages.is_empty() { 30000 } else { 15000 };
    let chars: Vec<char> = transcript.chars().collect();
    if chars.len() <= budget {
        user.push_str(&format!("【纠错后转写全文】\n{transcript}"));
    } else if stages.is_empty() {
        user.push_str(&format!("【纠错后转写（仅保留最后 {budget} 字）】\n{}", chars[chars.len() - budget..].iter().collect::<String>()));
    }
    let _ = settings;
    let report = llm.chat(model, &t.report_system(), &user, 0.3, 4096, Duration::from_secs(180)).await?;
    db.exec("UPDATE sessions SET report=? WHERE id=?", &[&report, &sid])?;
    Ok(report)
}

// ---------------------------------------------------------------- 运行时上下文

struct Ctx {
    db: Arc<Db>,
    emit: Emitter,
    llm: Llm,
    settings: Settings,
    terms: Vec<String>,
    sid: i64,
    template: Template,
    model: Arc<Mutex<String>>,
    seq: AtomicI64,
    stage: AMutex<StageState>,
    pending: Mutex<Vec<JoinHandle<()>>>,
}

impl Ctx {
    fn notice(&self, level: &str, text: &str) {
        (self.emit)("notice", json!({"level": level, "text": text}));
    }

    fn on_partial(&self, text: &str) {
        (self.emit)("asr-partial", json!({"text": text}));
    }

    fn on_final(self: &Arc<Self>, raw: String, begin_ms: i64, end_ms: i64) {
        let cs = &self.settings;
        let fast = raw.clone();
        let stage = "raw";
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let id = match self.db.insert(
            "INSERT INTO segments(session_id,seq,begin_ms,end_ms,raw,text,stage) VALUES(?,?,?,?,?,?,?)",
            &[&self.sid, &seq, &begin_ms, &end_ms, &raw, &fast, &stage],
        ) {
            Ok(id) => id,
            Err(e) => return self.notice("error", &format!("保存转写失败：{e}")),
        };
        (self.emit)("asr-partial", json!({"text": ""}));
        (self.emit)("segment", json!({"id": id, "seq": seq, "begin_ms": begin_ms, "end_ms": end_ms, "raw": raw, "text": fast, "stage": stage, "pending": cs.correct_enabled}));
        if !cs.correct_enabled {
            return;
        }
        let ctx = self.clone();
        let handle = tokio::spawn(async move {
            let history: Vec<String> = ctx
                .db
                .query("SELECT text FROM segments WHERE session_id=? AND id<? ORDER BY id DESC LIMIT 12", &[&ctx.sid, &id])
                .unwrap_or_default()
                .iter()
                .rev()
                .filter_map(|r| r["text"].as_str().map(String::from))
                .collect();
            let cs = &ctx.settings;
            let ans = context_correct(&ctx.llm, &cs.correct_model, &cs.correct_strength, &ctx.terms, &history, &fast, cs.correct_timeout_ms).await;
            let (text, stage) = match ans {
                Some(c) => (c, "context"),
                None => (fast.clone(), stage),
            };
            let _ = ctx.db.exec("UPDATE segments SET text=?, stage=? WHERE id=?", &[&text, &stage, &id]);
            (ctx.emit)("segment-corrected", json!({"id": id, "text": text, "stage": stage}));
        });
        self.pending.lock().unwrap().push(handle);
    }

    async fn summarize(&self) -> Option<Value> {
        let mut st = self.stage.lock().await;
        let segs = self.db.query("SELECT id,begin_ms,end_ms,text FROM segments WHERE session_id=? AND id>? ORDER BY id", &[&self.sid, &st.last_seg_id]).ok()?;
        if segs.is_empty() {
            return None;
        }
        let prev = self.db.query("SELECT content FROM stages WHERE session_id=? ORDER BY idx DESC LIMIT 3", &[&self.sid]).unwrap_or_default();
        let mut user = String::new();
        if !prev.is_empty() {
            user.push_str("【前文摘要（供衔接，勿重复）】\n");
            for p in prev.iter().rev() {
                user.push_str(p["content"].as_str().unwrap_or(""));
                user.push_str("\n\n");
            }
        }
        let from = segs[0]["begin_ms"].as_i64().unwrap_or(0);
        let to = segs.last().unwrap()["end_ms"].as_i64().unwrap_or(from);
        user.push_str(&format!("【本段转写 {} – {}】\n{}", fmt_ms(from), fmt_ms(to), lines(&segs)));
        let model = self.model.lock().unwrap().clone();
        (self.emit)("stage-start", json!({"from_ms": from, "to_ms": to}));
        match self.llm.chat(&model, &self.template.stage_system(self.template.interval_min), &user, 0.3, 1500, Duration::from_secs(90)).await {
            Ok(content) => {
                let idx = st.idx + 1;
                let created = now_ms();
                let id = self.db.insert("INSERT INTO stages(session_id,idx,from_ms,to_ms,content,model,created_at) VALUES(?,?,?,?,?,?,?)", &[&self.sid, &idx, &from, &to, &content, &model, &created]).ok()?;
                st.idx = idx;
                st.last_seg_id = segs.last().unwrap()["id"].as_i64().unwrap_or(st.last_seg_id);
                let v = json!({"id": id, "idx": idx, "from_ms": from, "to_ms": to, "content": content, "model": model});
                (self.emit)("stage", v.clone());
                Some(v)
            }
            Err(e) => {
                self.notice("error", &format!("阶段整理失败（下次会重试）：{e}"));
                (self.emit)("stage-fail", json!({}));
                None
            }
        }
    }
}

// ---------------------------------------------------------------- 引擎

struct Active {
    session_id: i64,
    stop: watch::Sender<bool>,
    paused: Arc<AtomicBool>,
    summarize: mpsc::UnboundedSender<()>,
    model: Arc<Mutex<String>>,
}

pub struct Engine {
    pub db: Arc<Db>,
    pub emit: Emitter,
    active: Mutex<Option<Active>>,
}

impl Engine {
    pub fn new(db: Arc<Db>, emit: Emitter) -> Arc<Self> {
        // 上次异常退出遗留的录制中会议，标记为已结束
        let _ = db.exec("UPDATE sessions SET status='done' WHERE status IN ('recording','finalizing')", &[]);
        Arc::new(Engine { db, emit, active: Mutex::new(None) })
    }

    pub fn active_id(&self) -> Option<i64> {
        self.active.lock().unwrap().as_ref().map(|a| a.session_id)
    }

    pub fn pause(&self, paused: bool) {
        if let Some(a) = self.active.lock().unwrap().as_ref() {
            a.paused.store(paused, Ordering::Relaxed);
            (self.emit)("session-status", json!({"state": if paused { "paused" } else { "recording" }, "session_id": a.session_id}));
        }
    }

    pub fn stop(&self) {
        if let Some(a) = self.active.lock().unwrap().as_ref() {
            let _ = a.stop.send(true);
        }
    }

    pub fn summarize_now(&self) {
        if let Some(a) = self.active.lock().unwrap().as_ref() {
            let _ = a.summarize.send(());
        }
    }

    pub fn set_model(&self, model: &str) {
        if let Some(a) = self.active.lock().unwrap().as_ref() {
            *a.model.lock().unwrap() = model.to_string();
        }
    }

    pub async fn start(self: &Arc<Self>, title: String, template_id: Option<i64>, audio_file: Option<String>) -> Result<Value> {
        if self.active_id().is_some() {
            return Err(anyhow!("已有会议正在录制"));
        }
        let settings = Settings::load(&self.db);
        if settings.api_key.is_empty() {
            return Err(anyhow!("未配置百炼密钥：请在「设置」中填写，或写入 .env 的 BAILIAN_API_KEY"));
        }
        let trow = match template_id {
            Some(id) => self.db.one("SELECT * FROM templates WHERE id=?", &[&id])?,
            None => None,
        };
        let trow = match trow {
            Some(r) => r,
            None => self.db.one("SELECT * FROM templates ORDER BY id LIMIT 1", &[])?.ok_or_else(|| anyhow!("没有可用的提示词模板"))?,
        };
        let template = Template::from_row(&trow);
        let started = now_ms();
        let title = if title.trim().is_empty() { format!("会议 {}", fmt_datetime(started)) } else { title };
        let sid = self.db.insert(
            "INSERT INTO sessions(title,template_id,template_name,template_snapshot,started_at,status,asr_model,llm_model) VALUES(?,?,?,?,?, 'recording',?,?)",
            &[&title, &trow["id"].as_i64().unwrap_or(0), &template.name, &trow.to_string(), &started, &settings.asr_model, &settings.summary_model],
        )?;

        let (audio_tx, audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (stop_tx, stop_rx) = watch::channel(false);
        let paused = Arc::new(AtomicBool::new(false));
        let emit = self.emit.clone();
        let level: audio::LevelFn = Arc::new(move |v| emit("level", json!({"v": v})));
        let mut mic = None;
        let mut warnings: Vec<String> = vec![];
        if let Some(path) = audio_file {
            let pcm = audio::read_wav_pcm(&path).map_err(|e| { let _ = self.db.exec("DELETE FROM sessions WHERE id=?", &[&sid]); e })?;
            tokio::spawn(audio::feed_pcm(pcm, audio_tx, paused.clone(), level, stop_rx.clone()));
        } else {
            match audio::start_capture(audio::Source::parse(&settings.audio_source), &settings.mic_device, audio_tx, paused.clone(), level) {
                Ok((m, w)) => {
                    mic = Some(m);
                    warnings = w;
                }
                Err(e) => {
                    let _ = self.db.exec("DELETE FROM sessions WHERE id=?", &[&sid]);
                    return Err(anyhow!("无法开始采集音频：{e}（请在系统设置中允许麦克风 / 系统音频录制权限）"));
                }
            }
        }
        let (sum_tx, sum_rx) = mpsc::unbounded_channel::<()>();
        let model = Arc::new(Mutex::new(settings.summary_model.clone()));
        *self.active.lock().unwrap() = Some(Active { session_id: sid, stop: stop_tx, paused: paused.clone(), summarize: sum_tx, model: model.clone() });

        let ctx = Arc::new(Ctx {
            db: self.db.clone(),
            emit: self.emit.clone(),
            llm: Llm::new(settings.llm_url(), settings.api_key.clone()),
            terms: load_terms(&self.db, trow["id"].as_i64().unwrap_or(0)),
            settings,
            sid,
            template,
            model,
            seq: AtomicI64::new(0),
            stage: AMutex::new(StageState { last_seg_id: 0, idx: 0 }),
            pending: Mutex::new(vec![]),
        });
        (self.emit)("session-status", json!({"state": "recording", "session_id": sid}));
        for w in warnings {
            (self.emit)("notice", json!({"level": "warn", "text": w}));
        }
        let engine = self.clone();
        tokio::spawn(async move {
            run(ctx.clone(), audio_rx, stop_rx, sum_rx, started).await;
            drop(mic);
            *engine.active.lock().unwrap() = None;
            (engine.emit)("session-status", json!({"state": "done", "session_id": sid}));
        });
        Ok(json!({"id": sid, "title": title, "started_at": started, "template_name": trow["name"]}))
    }
}

pub fn fmt_datetime(ms: i64) -> String {
    // 不引入时间库：按 UTC+8 粗略格式化仅用于默认标题
    let secs = ms / 1000 + 8 * 3600;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil(days);
    format!("{y}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

async fn run(ctx: Arc<Ctx>, mut audio_rx: mpsc::UnboundedReceiver<Vec<u8>>, mut stop_rx: watch::Receiver<bool>, mut sum_rx: mpsc::UnboundedReceiver<()>, started_ms: i64) {
    let t0 = Instant::now();
    // 定时阶段整理
    let period = Duration::from_secs(ctx.template.interval_min * 60);
    let sum_ctx = ctx.clone();
    let mut sum_stop = stop_rx.clone();
    let summarizer = tokio::spawn(async move {
        let mut iv = tokio::time::interval(period);
        iv.tick().await;
        loop {
            tokio::select! {
                _ = iv.tick() => {}
                m = sum_rx.recv() => if m.is_none() { break },
                _ = sum_stop.changed() => break,
            }
            sum_ctx.summarize().await;
        }
    });

    let params = AsrParams { url: ctx.settings.asr_url(), api_key: ctx.settings.api_key.clone(), model: ctx.settings.asr_model.clone(), lang: ctx.settings.asr_lang.clone() };
    let mut failures = 0;
    loop {
        let offset = t0.elapsed().as_millis() as i64;
        let cb_ctx = ctx.clone();
        let on_event = move |ev: AsrEvent| match ev {
            AsrEvent::Partial { text } => cb_ctx.on_partial(&text),
            AsrEvent::Final { text, begin_ms, end_ms } => cb_ctx.on_final(text, offset + begin_ms, offset + end_ms),
        };
        match asr::run_connection(&params, &mut audio_rx, &mut stop_rx, &on_event).await {
            Ok(ConnEnd::Stopped) => break,
            Ok(ConnEnd::Dropped(msg)) => {
                failures += 1;
                ctx.notice("warn", &format!("语音识别连接中断，正在重连：{msg}"));
                (ctx.emit)("session-status", json!({"state": "reconnecting", "session_id": ctx.sid}));
                if failures > 5 {
                    ctx.notice("error", "语音识别多次重连失败，已结束录制");
                    break;
                }
            }
            Err(e) => {
                failures += 1;
                ctx.notice("error", &format!("语音识别启动失败：{e}"));
                if failures > 3 {
                    break;
                }
            }
        }
        if *stop_rx.borrow() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        (ctx.emit)("session-status", json!({"state": "recording", "session_id": ctx.sid}));
    }

    // 收尾：等待纠错 → 最后一段阶段整理 → 最终总结
    (ctx.emit)("session-status", json!({"state": "finalizing", "session_id": ctx.sid}));
    let handles: Vec<JoinHandle<()>> = std::mem::take(&mut *ctx.pending.lock().unwrap());
    let _ = tokio::time::timeout(Duration::from_secs(8), async {
        for h in handles {
            let _ = h.await;
        }
    })
    .await;
    let _ = tokio::time::timeout(Duration::from_secs(100), summarizer).await;
    let duration = t0.elapsed().as_millis() as i64;
    let seg_count = ctx.seq.load(Ordering::SeqCst);
    if seg_count > 0 {
        ctx.summarize().await;
        let model = ctx.model.lock().unwrap().clone();
        match generate_report(&ctx.db, &ctx.llm, &ctx.settings, ctx.sid, &model).await {
            Ok(r) => (ctx.emit)("report", json!({"session_id": ctx.sid, "report": r})),
            Err(e) => ctx.notice("error", &format!("生成总结失败，可在历史中重新生成：{e}")),
        }
    }
    let _ = ctx.db.exec("UPDATE sessions SET status='done', ended_at=?, duration_ms=? WHERE id=?", &[&(started_ms + duration), &duration, &ctx.sid]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_follow_book_switch_and_binding() {
        let path = std::env::temp_dir().join(format!("minutes-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            // 旧版库：只有词条，没有词库
            let c = rusqlite::Connection::open(&path).unwrap();
            c.execute_batch("CREATE TABLE lexicon (id INTEGER PRIMARY KEY AUTOINCREMENT, term TEXT NOT NULL, misspellings TEXT NOT NULL DEFAULT '', weight TEXT NOT NULL DEFAULT 'mid', enabled INTEGER NOT NULL DEFAULT 1, note TEXT NOT NULL DEFAULT ''); INSERT INTO lexicon(term) VALUES('Kafka');").unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(load_terms(&db, 1), vec!["Kafka"]); // 迁入「默认词库」，通用
        let b = db.insert("INSERT INTO lexicon_books(name) VALUES('面试')", &[]).unwrap();
        db.exec("INSERT INTO lexicon(term,book_id) VALUES('灰度发布',?),('kafka',?)", &[&b, &b]).unwrap();
        db.exec("INSERT INTO template_books(template_id,book_id) VALUES(1,?)", &[&b]).unwrap();
        assert_eq!(load_terms(&db, 1), vec!["Kafka", "灰度发布"]); // 绑定 + 通用，去重
        assert_eq!(load_terms(&db, 2), vec!["Kafka"]); // 未绑定模板 2
        db.exec("UPDATE lexicon_books SET enabled=0 WHERE id=?", &[&b]).unwrap();
        assert_eq!(load_terms(&db, 1), vec!["Kafka"]); // 停用词库
        drop(db);
        let _ = std::fs::remove_file(&path);
    }
}
