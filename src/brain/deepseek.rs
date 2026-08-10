use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};

use super::BrainEvent;
use crate::config::DeepSeekCfg;

/// DeepSeek(OpenAI 兼容 API)流式聊天。保留最近几轮上下文。
pub struct DeepSeekBrain {
    client: Client,
    cfg: DeepSeekCfg,
    history: Vec<Value>,
}

impl DeepSeekBrain {
    pub fn new(cfg: &DeepSeekCfg) -> Self {
        Self { client: Client::new(), cfg: cfg.clone(), history: Vec::new() }
    }

    pub async fn run(&mut self, text: &str) -> Vec<BrainEvent> {
        if self.cfg.api_key.trim().is_empty() {
            return vec![BrainEvent::Err("未配置 DEEPSEEK_API_KEY(环境变量或 config.toml)".into())];
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
            Err(e) => return vec![BrainEvent::Err(format!("请求 DeepSeek 失败: {e}"))],
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return vec![BrainEvent::Err(format!("DeepSeek API {status}: {body}"))];
        }

        let mut events = Vec::new();
        let mut full = String::new();
        let mut buf: Vec<u8> = Vec::new();
        let mut stream = resp.bytes_stream();
        let t0 = std::time::Instant::now();
        let mut first_token_at: Option<std::time::Instant> = None;

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(c) => buf.extend_from_slice(&c),
                Err(e) => {
                    events.push(BrainEvent::Err(format!("读取回复流失败: {e}")));
                    break;
                }
            }
            // 按行解析 SSE
            loop {
                let Some(pos) = buf.iter().position(|&b| b == b'\n') else { break };
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line).trim().to_string();
                let Some(data) = line.strip_prefix("data:") else { continue };
                let data = data.trim();
                if data == "[DONE]" {
                    break;
                }
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if let Some(delta) = v["choices"][0]["delta"]["content"].as_str() {
                        if !delta.is_empty() {
                            if first_token_at.is_none() {
                                first_token_at = Some(std::time::Instant::now());
                            }
                            full.push_str(delta);
                            events.push(BrainEvent::Delta(delta.to_string()));
                        }
                    }
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
            if events.is_empty() {
                events.push(BrainEvent::Err("模型返回为空".into()));
            }
            return events;
        }

        // 记录上下文
        self.history.push(json!({ "role": "user", "content": text }));
        self.history.push(json!({ "role": "assistant", "content": reply }));
        if self.history.len() > 16 {
            self.history.drain(..self.history.len() - 16);
        }

        events.push(BrainEvent::Done(reply));
        events
    }
}
