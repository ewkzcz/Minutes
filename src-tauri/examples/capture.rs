//! 音频采集自检：cargo run --example capture -- [mic|system|both]，播放声音时观察系统音量条。
use meeting_notes_lib::audio::{start_capture, Source};
use std::sync::{atomic::AtomicBool, Arc};

#[tokio::main]
async fn main() {
    let src = Source::parse(&std::env::args().nth(1).unwrap_or("both".into()));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let level: meeting_notes_lib::audio::LevelFn = Arc::new(|_| {});
    match start_capture(src, "", tx, Arc::new(AtomicBool::new(false)), level) {
        Ok((h, w)) => {
            println!("ok {src:?} warnings={w:?}");
            let mut peak = 0i32;
            let mut n = 0;
            let t = std::time::Instant::now();
            while t.elapsed().as_secs() < 6 {
                if let Some(c) = rx.recv().await {
                    n += 1;
                    for b in c.chunks_exact(2) { peak = peak.max((i16::from_le_bytes([b[0], b[1]]) as i32).abs()); }
                }
            }
            println!("chunks={n} peak={peak}");
            drop(h);
        }
        Err(e) => println!("ERR {e}"),
    }
}
