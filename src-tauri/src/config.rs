//! 配置：应用设置（数据库）优先，缺省回退到 .env 中的百炼变量与内置默认值。

use crate::db::Db;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const DEFAULT_ASR_MODELS: &[&str] = &[
    "qwen-audio-3.1-asr-flash-streaming",
    "qwen-audio-3.0-asr-flash-streaming",
    "fun-asr-realtime",
];
pub const DEFAULT_LLM_MODELS: &[&str] = &["qwen3.8-flash", "qwen3.7-flash-2026-07-15", "qwen-plus", "qwen-max", "qwen-turbo"];

/// 依次读取多个位置的 .env（已存在的环境变量不覆盖）。
pub fn load_env(extra: &[PathBuf]) {
    let mut paths: Vec<PathBuf> = vec![PathBuf::from(".env")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join(".env"));
        }
    }
    if let Some(dir) = option_env!("CARGO_MANIFEST_DIR") {
        paths.push(Path::new(dir).join("../.env"));
    }
    paths.extend(extra.iter().cloned());
    for p in paths {
        if p.is_file() {
            let _ = dotenvy::from_path(&p);
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Settings {
    pub api_key: String,
    pub workspace_id: String,
    pub llm_base_url: String,
    pub asr_model: String,
    pub asr_lang: String,
    pub summary_model: String,
    pub correct_model: String,
    pub correct_enabled: bool,
    pub correct_strength: String,
    pub correct_timeout_ms: u64,
    pub interval_min: u64,
    pub mic_device: String,
    pub audio_source: String,
    pub theme: String,
}

fn s(db: &Db, key: &str, default: &str) -> String {
    db.setting(key).filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

impl Settings {
    pub fn load(db: &Db) -> Self {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        Settings {
            api_key: s(db, "api_key", &env("BAILIAN_API_KEY")),
            workspace_id: s(db, "workspace_id", &env("BAILIAN_WORKSPACE_ID")),
            llm_base_url: s(db, "llm_base_url", ""),
            asr_model: s(db, "asr_model", DEFAULT_ASR_MODELS[0]),
            asr_lang: s(db, "asr_lang", "zh-en"),
            summary_model: s(db, "summary_model", DEFAULT_LLM_MODELS[0]),
            correct_model: s(db, "correct_model", DEFAULT_LLM_MODELS[1]),
            correct_enabled: s(db, "correct_enabled", "1") == "1",
            correct_strength: s(db, "correct_strength", "balanced"),
            correct_timeout_ms: s(db, "correct_timeout_ms", "2500").parse().unwrap_or(2500),
            interval_min: s(db, "interval_min", "5").parse().unwrap_or(5).clamp(1, 60),
            mic_device: s(db, "mic_device", ""),
            audio_source: s(db, "audio_source", "both"),
            theme: s(db, "theme", "system"),
        }
    }

    /// 语音识别 WebSocket 地址
    pub fn asr_url(&self) -> String {
        let host = if self.workspace_id.is_empty() {
            "dashscope.aliyuncs.com".to_string()
        } else {
            format!("{}.cn-beijing.maas.aliyuncs.com", self.workspace_id)
        };
        format!("wss://{host}/api-ws/v1/inference")
    }

    /// 对话模型的 OpenAI 兼容地址
    pub fn llm_url(&self) -> String {
        if !self.llm_base_url.is_empty() {
            return self.llm_base_url.trim_end_matches('/').to_string();
        }
        let host = if self.workspace_id.is_empty() {
            "dashscope.aliyuncs.com".to_string()
        } else {
            format!("{}.cn-beijing.maas.aliyuncs.com", self.workspace_id)
        };
        format!("https://{host}/compatible-mode/v1")
    }
}
