use std::time::Duration;

use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};

use super::BrainEvent;
use crate::config::DeepSeekCfg;

/// DeepSeek(OpenAI 兼容 API)流式聊天。保留最近几轮上下文。
/// run_streaming 把增量事件实时推给调用方(首字一到即可开始合成语音)。
pub struct DeepSeekBrain {
    client: Client,
    cfg: DeepSeekCfg,
    history: Vec<Value>,
}

/// 连接建立超时。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// 流式响应相邻数据块的最大间隔: 超过则判定链路挂死(避免永远卡在思考中)。
/// 生成中的块间隔远小于此值,长回复不受影响。
const STREAM_CHUNK_TIMEOUT: Duration = Duration::from_secs(60);

impl DeepSeekBrain {
    pub fn new(cfg: &DeepSeekCfg) -> Self {
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self { client, cfg: cfg.clone(), history: Vec::new() }
    }

    pub async fn run_streaming(&mut self, text: &str, tx: flume::Sender<BrainEvent>) {
        let send = |e: BrainEvent| {
            let _ = tx.send(e);
        };

        if self.cfg.api_key.trim().is_empty() {
            send(BrainEvent::Err("未配置 DEEPSEEK_API_KEY(环境变量或 config.toml)".into()));
            return;
        }

        let mut messages: Vec<Value> = vec![json!({
            "role": "system",
            "content": self.cfg.system_prompt,
        })];
        messages.extend(self.history.iter().cloned());
        messages.push(json!({ "role": "user", "content": text }));

        let url = format!("{}/chat/completions", self.cfg.base_url.trim_end_matches('/'));
        let body = json!({
            "model": self.cfg.model,
            "messages": messages,
            "stream": true,
            "temperature": 0.8,
        });

        let resp = match self
            .client
            .post(&url)
            .bearer_auth(self.cfg.api_key.trim())
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                send(BrainEvent::Err(format!("请求 DeepSeek 失败: {e}")));
                return;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            send(BrainEvent::Err(format!("DeepSeek API {status}: {body}")));
            return;
        }

        let mut full = String::new();
        let mut buf: Vec<u8> = Vec::new();
        let mut stream = resp.bytes_stream();
        let t0 = std::time::Instant::now();
        let mut first_token_at: Option<std::time::Instant> = None;

        // 逐块读取,带超时: 连接后长时间收不到任何数据视为链路挂死
        // (reqwest 默认无超时,旧代码会永远卡在思考中)。
        loop {
            let chunk = match tokio::time::timeout(STREAM_CHUNK_TIMEOUT, stream.next()).await {
                Ok(Some(Ok(c))) => c,
                Ok(Some(Err(e))) => {
                    send(BrainEvent::Err(format!("读取回复流失败: {e}")));
                    break;
                }
                Ok(None) => break, // 流正常结束
                Err(_) => {
                    send(BrainEvent::Err(format!(
                        "DeepSeek 响应超时({}s 无数据)",
                        STREAM_CHUNK_TIMEOUT.as_secs()
                    )));
                    break;
                }
            };
            buf.extend_from_slice(&chunk);
            // 按行解析 SSE
            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line).trim().to_string();
                let Some(data) = line.strip_prefix("data:") else { continue };
                let data = data.trim();
                if data == "[DONE]" {
                    break;
                }
                if let Ok(v) = serde_json::from_str::<Value>(data)
                    && let Some(delta) = v["choices"][0]["delta"]["content"].as_str()
                    && !delta.is_empty()
                {
                    if first_token_at.is_none() {
                        first_token_at = Some(std::time::Instant::now());
                    }
                    full.push_str(delta);
                    send(BrainEvent::Delta(delta.to_string()));
                }
            }
        }
        if let Some(ft) = first_token_at {
            tracing::info!(
                "LATENCY LLM 首字 {:.2}s 全文 {:.2}s ({}字)",
                ft.duration_since(t0).as_secs_f32(),
                t0.elapsed().as_secs_f32(),
                full.chars().count()
            );
        }

        let reply = full.trim().to_string();
        if reply.is_empty() {
            return; // 错误事件已在上游推送
        }

        // 记录上下文
        self.history.push(json!({ "role": "user", "content": text }));
        self.history.push(json!({ "role": "assistant", "content": reply }));
        if self.history.len() > 16 {
            self.history.drain(..self.history.len() - 16);
        }

        send(BrainEvent::Done(reply));
    }
}
