//! 无界面联调：用 WAV 文件走完整链路（识别 → 纠错 → 阶段整理 → 总结）。
//! 用法：cargo run --example headless -- /path/to/16k-mono.wav [录制秒数]
use meeting_notes_lib::{config, db::Db, session::Engine};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let wav = args.next().expect("需要 WAV 路径");
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(30);
    config::load_env(&[]);
    let path = std::env::temp_dir().join("meeting-headless.db");
    let _ = std::fs::remove_file(&path);
    let db = Arc::new(Db::open(&path).unwrap());
    db.exec("INSERT INTO lexicon(term,misspellings,weight) VALUES('Kafka','卡夫卡','mid')", &[]).unwrap();
    let emit: meeting_notes_lib::session::Emitter = Arc::new(|name, v| {
        if name != "level" && name != "asr-partial" {
            println!("[{name}] {v}");
        }
    });
    let engine = Engine::new(db.clone(), emit);
    let r = engine.start("联调".into(), None, Some(wav)).await.unwrap();
    println!("started: {r}");
    tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
    engine.summarize_now();
    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
    engine.stop();
    while engine.active_id().is_some() {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    let s = db.one("SELECT status,duration_ms,report FROM sessions", &[]).unwrap().unwrap();
    println!("== session {s}");
}
