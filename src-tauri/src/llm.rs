//! 百炼对话模型（OpenAI 兼容协议）客户端。

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct Llm {
    base: String,
    key: String,
    client: reqwest::Client,
}

impl Llm {
    pub fn new(base: String, key: String) -> Self {
        Llm { base, key, client: reqwest::Client::new() }
    }

    /// 非流式对话，返回完整文本。
    pub async fn chat(&self, model: &str, system: &str, user: &str, temperature: f32, max_tokens: u32, timeout: Duration) -> Result<String> {
        if self.key.is_empty() {
            return Err(anyhow!("未配置百炼密钥，请在设置中填写或写入 .env"));
        }
        let body = json!({
            "model": model,
            "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
            "temperature": temperature,
            "max_tokens": max_tokens,
            "stream": false,
            "enable_thinking": false
        });
        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base))
            .bearer_auth(&self.key)
            .timeout(timeout)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow!("HTTP {status}: {}", text.chars().take(300).collect::<String>()));
        }
        let v: Value = serde_json::from_str(&text)?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.trim().to_string())
            .ok_or_else(|| anyhow!("模型未返回内容"))
    }
}
