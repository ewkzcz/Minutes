//! 百炼实时语音识别（WebSocket 流式，run-task / finish-task 协议）。

use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::{mpsc::UnboundedReceiver, watch};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

pub struct AsrParams {
    pub url: String,
    pub api_key: String,
    pub model: String,
    pub lang: String,
}

pub enum AsrEvent {
    Partial { text: String },
    Final { text: String, begin_ms: i64, end_ms: i64 },
}

pub enum ConnEnd {
    Stopped,
    Dropped(String),
}

fn hints(lang: &str) -> Value {
    match lang {
        "zh" => json!(["zh"]),
        "en" => json!(["en"]),
        _ => json!(["zh", "en"]),
    }
}

fn header(action: &str, task_id: &str) -> Value {
    json!({"action": action, "task_id": task_id, "streaming": "duplex"})
}

fn task_id() -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    format!("{:032x}", n ^ (std::process::id() as u128) << 64)
}

fn parameters(model: &str, lang: &str) -> Value {
    let mut p = json!({"format": "pcm", "sample_rate": 16000, "language_hints": hints(lang)});
    if model.starts_with("qwen-audio-3.1-") {
        p["vad_model"] = json!("near_meeting_16k");
    } else {
        p["max_sentence_silence"] = json!(800);
    }
    p
}

/// 解析一条服务端消息；返回 Some(Ok(event)) / Some(Err) 表示失败 / None 表示无需处理。
fn parse(text: &str) -> Option<Result<Option<AsrEvent>, String>> {
    let v: Value = serde_json::from_str(text).ok()?;
    match v["header"]["event"].as_str()? {
        "task-failed" => Some(Err(v["header"]["error_message"].as_str().unwrap_or("识别失败").to_string())),
        "result-generated" => {
            let sent = &v["payload"]["output"]["sentence"];
            let t = sent["text"].as_str().unwrap_or("").trim().to_string();
            if t.is_empty() {
                return Some(Ok(None));
            }
            if sent["sentence_end"].as_bool().unwrap_or(false) {
                Some(Ok(Some(AsrEvent::Final {
                    text: t,
                    begin_ms: sent["begin_time"].as_i64().unwrap_or(0),
                    end_ms: sent["end_time"].as_i64().unwrap_or(0),
                })))
            } else {
                Some(Ok(Some(AsrEvent::Partial { text: t })))
            }
        }
        _ => None,
    }
}

/// 建立一次识别连接并持续推流，直到用户停止或连接中断。
pub async fn run_connection(
    p: &AsrParams,
    audio: &mut UnboundedReceiver<Vec<u8>>,
    stop: &mut watch::Receiver<bool>,
    on_event: &(dyn Fn(AsrEvent) + Send + Sync),
) -> Result<ConnEnd> {
    let mut req = p.url.as_str().into_client_request()?;
    req.headers_mut().insert("Authorization", format!("bearer {}", p.api_key).parse()?);
    let (ws, _) = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async(req))
        .await
        .map_err(|_| anyhow!("连接语音识别服务超时"))??;
    let (mut tx, mut rx) = ws.split();
    let tid = task_id();
    tx.send(Message::Text(
        json!({"header": header("run-task", &tid), "payload": {
            "task_group": "audio", "task": "asr", "function": "recognition", "model": p.model,
            "parameters": parameters(&p.model, &p.lang), "input": {}}})
        .to_string(),
    ))
    .await?;
    // 等待 task-started
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), rx.next()).await.map_err(|_| anyhow!("等待任务启动超时"))?;
        match msg {
            Some(Ok(Message::Text(t))) => {
                let v: Value = serde_json::from_str(&t)?;
                match v["header"]["event"].as_str() {
                    Some("task-started") => break,
                    Some("task-failed") => return Err(anyhow!("{}", v["header"]["error_message"].as_str().unwrap_or("启动失败"))),
                    _ => {}
                }
            }
            Some(Ok(_)) => {}
            _ => return Err(anyhow!("连接在任务启动前关闭")),
        }
    }
    loop {
        tokio::select! {
            chunk = audio.recv() => match chunk {
                Some(c) => { if let Err(e) = tx.send(Message::Binary(c)).await { return Ok(ConnEnd::Dropped(e.to_string())); } }
                None => break,
            },
            msg = rx.next() => match msg {
                Some(Ok(Message::Text(t))) => match parse(&t) {
                    Some(Err(e)) => return Ok(ConnEnd::Dropped(e)),
                    Some(Ok(Some(ev))) => on_event(ev),
                    _ => {}
                },
                Some(Ok(Message::Close(_))) | None => return Ok(ConnEnd::Dropped("服务端关闭了连接".into())),
                Some(Err(e)) => return Ok(ConnEnd::Dropped(e.to_string())),
                _ => {}
            },
            _ = stop.changed() => break,
        }
    }
    // 收尾：finish-task，并在限定时间内收完剩余结果
    let _ = tx.send(Message::Text(json!({"header": header("finish-task", &tid), "payload": {"input": {}}}).to_string())).await;
    let _ = tokio::time::timeout(Duration::from_secs(4), async {
        while let Some(Ok(m)) = rx.next().await {
            if let Message::Text(t) = m {
                let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                if v["header"]["event"] == "task-finished" {
                    break;
                }
                if let Some(Ok(Some(ev))) = parse(&t) {
                    on_event(ev);
                }
            }
        }
    })
    .await;
    let _ = tx.close().await;
    Ok(ConnEnd::Stopped)
}

/// 连通性检查：建立连接、启动任务、立即结束。
pub async fn probe(p: &AsrParams) -> Result<()> {
    let mut req = p.url.as_str().into_client_request()?;
    req.headers_mut().insert("Authorization", format!("bearer {}", p.api_key).parse()?);
    let (ws, _) = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async(req))
        .await
        .map_err(|_| anyhow!("连接超时"))??;
    let (mut tx, mut rx) = ws.split();
    let tid = task_id();
    tx.send(Message::Text(
        json!({"header": header("run-task", &tid), "payload": {
            "task_group": "audio", "task": "asr", "function": "recognition", "model": p.model,
            "parameters": parameters(&p.model, &p.lang), "input": {}}})
        .to_string(),
    ))
    .await?;
    let res = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(m) = rx.next().await {
            if let Message::Text(t) = m? {
                let v: Value = serde_json::from_str(&t)?;
                match v["header"]["event"].as_str() {
                    Some("task-started") => return Ok(()),
                    Some("task-failed") => return Err(anyhow!("{}", v["header"]["error_message"].as_str().unwrap_or("任务失败"))),
                    _ => {}
                }
            }
        }
        Err(anyhow!("连接被关闭"))
    })
    .await
    .map_err(|_| anyhow!("等待任务启动超时"))?;
    let _ = tx.send(Message::Text(json!({"header": header("finish-task", &tid), "payload": {"input": {}}}).to_string())).await;
    let _ = tx.close().await;
    res
}
