//! 会议纪要：Tauri 桌面应用。语音识别、纠错、总结与存储全部在桌面端进程内完成。

pub mod asr;
pub mod audio;
pub mod commands;
pub mod config;
pub mod correct;
pub mod db;
pub mod export;
pub mod llm;
pub mod session;

use commands::App;
use std::sync::Arc;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            config::load_env(&[data_dir.join(".env")]);
            let db = Arc::new(db::Db::open(&data_dir.join("meeting.db"))?);
            let handle = app.handle().clone();
            let emit: session::Emitter = Arc::new(move |name, payload| {
                let _ = handle.emit(name, payload);
            });
            let engine = session::Engine::new(db.clone(), emit);
            app.manage(App { engine, db });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::test_connection,
            commands::start_session,
            commands::pause_session,
            commands::stop_session,
            commands::summarize_now,
            commands::set_live_model,
            commands::sessions_list,
            commands::session_get,
            commands::session_update,
            commands::sessions_delete,
            commands::regenerate_report,
            commands::session_export,
            commands::session_markdown,
            commands::books_list,
            commands::book_save,
            commands::book_toggle,
            commands::book_delete,
            commands::book_bind,
            commands::lexicon_list,
            commands::lexicon_add,
            commands::lexicon_rename,
            commands::lexicon_toggle,
            commands::lexicon_delete,
            commands::lexicon_test,
            commands::templates_list,
            commands::template_save,
            commands::template_delete,
            commands::templates_restore,
        ])
        .run(tauri::generate_context!())
        .expect("运行会议纪要应用失败");
}
