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
    /// 流式响应相邻数据块的最大间隔(测试可注入,生产用 STREAM_CHUNK_TIMEOUT)
    chunk_timeout: Duration,
    /// 响应头超时(测试可注入,生产用 HEADERS_TIMEOUT)
    headers_timeout: Duration,
}

/// 连接建立超时。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// 请求发出后等待响应头的超时: 覆盖代理/服务端"连接了但迟迟不回头"的挂死
/// (reqwest 的 connect_timeout 只覆盖 TCP 建连,代理吞请求时不会触发)。
/// 流式正文不受此限制(正文用块超时)。
const HEADERS_TIMEOUT: Duration = Duration::from_secs(15);
/// 流式响应相邻数据块的最大间隔: 超过则判定链路挂死(避免永远卡在思考中)。
/// 生成中的块间隔远小于此值,长回复不受影响。
const STREAM_CHUNK_TIMEOUT: Duration = Duration::from_secs(60);

impl DeepSeekBrain {
    pub fn new(cfg: &DeepSeekCfg) -> Self {
        Self::new_with_timeouts(cfg, CONNECT_TIMEOUT, HEADERS_TIMEOUT, STREAM_CHUNK_TIMEOUT)
    }

    /// 可注入超时(测试用): 生产路径走 new(),默认常量超时。
    pub(crate) fn new_with_timeouts(
        cfg: &DeepSeekCfg,
        connect: Duration,
        headers: Duration,
        chunk: Duration,
    ) -> Self {
        let client = Client::builder()
            .connect_timeout(connect)
            .build()
            .unwrap_or_default();
        Self {
            client,
            cfg: cfg.clone(),
            history: Vec::new(),
            chunk_timeout: chunk,
            headers_timeout: headers,
        }
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

        // 请求头超时: 代理/服务端连接了但迟迟不回响应头 → 报错退出
        // (reqwest 的 connect_timeout 不覆盖此阶段,旧代码会无限等待)。
        let resp = match tokio::time::timeout(
            self.headers_timeout,
            self.client
                .post(&url)
                .bearer_auth(self.cfg.api_key.trim())
                .json(&body)
                .send(),
        )
        .await
        {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                send(BrainEvent::Err(format!("请求 DeepSeek 失败: {e}")));
                return;
            }
            Err(_) => {
                send(BrainEvent::Err(format!(
                    "DeepSeek 请求超时({}s 未收到响应头)",
                    self.headers_timeout.as_secs()
                )));
                return;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = match tokio::time::timeout(self.headers_timeout, resp.text()).await {
                Ok(Ok(b)) => b,
                Ok(Err(_)) => String::new(),
                Err(_) => "<读取错误响应体超时>".into(),
            };
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
            let chunk = match tokio::time::timeout(self.chunk_timeout, stream.next()).await {
                Ok(Some(Ok(c))) => c,
                Ok(Some(Err(e))) => {
                    send(BrainEvent::Err(format!("读取回复流失败: {e}")));
                    break;
                }
                Ok(None) => break, // 流正常结束
                Err(_) => {
                    send(BrainEvent::Err(format!(
                        "DeepSeek 响应超时({}s 无数据)",
                        self.chunk_timeout.as_secs()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::BrainEvent;
    use crate::config::DeepSeekCfg;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn cfg_for(base: &str) -> DeepSeekCfg {
        DeepSeekCfg {
            base_url: base.into(),
            api_key: "test-key".into(),
            ..Default::default()
        }
    }

    async fn collect(b: &mut DeepSeekBrain, text: &str) -> Vec<BrainEvent> {
        let (tx, rx) = flume::unbounded();
        b.run_streaming(text, tx).await;
        rx.drain().collect()
    }

    /// 流数据块超时: 服务端发完一个 SSE 块后挂死,客户端必须在
    /// chunk 超时内报错退出(修复前 reqwest 无超时,会永远卡思考)。
    #[tokio::test]
    async fn stalled_stream_times_out_with_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf).await; // 吃掉请求头
            let body = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\n";
            sock.write_all(body.as_bytes()).await.unwrap();
            sock.flush().await.unwrap();
            // 之后不再写任何数据 → 挂死,等待客户端超时
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        });

        let cfg = cfg_for(&format!("http://{addr}"));
        let mut b = DeepSeekBrain::new_with_timeouts(
            &cfg,
            Duration::from_secs(2),
            Duration::from_secs(2),
            Duration::from_millis(800),
        );
        let t0 = std::time::Instant::now();
        let evs = collect(&mut b, "你好").await;
        assert!(
            evs.iter().any(|e| matches!(e, BrainEvent::Delta(d) if d == "你好")),
            "应先收到首块增量: {evs:?}"
        );
        assert!(
            evs.iter().any(|e| matches!(e, BrainEvent::Err(e) if e.contains("超时"))),
            "挂死流应报超时错误: {evs:?}"
        );
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "应在块超时(0.8s)附近退出,实际 {}s",
            t0.elapsed().as_secs_f32()
        );
    }

    /// 连接/响应头超时: 不可达地址(经系统代理)必须在超时内报错,
    /// 而不是无限等待。192.0.2.1 是 RFC 5737 保留测试网段,公网/内网均不可路由;
    /// 本机有系统代理时请求会经代理转发,代理吞请求正对应 HEADERS_TIMEOUT 路径。
    #[tokio::test]
    async fn unreachable_endpoint_errors_within_connect_timeout() {
        let cfg = cfg_for("http://192.0.2.1:9");
        let mut b = DeepSeekBrain::new_with_timeouts(
            &cfg,
            Duration::from_millis(800),
            Duration::from_millis(800),
            Duration::from_secs(5),
        );
        let t0 = std::time::Instant::now();
        let evs = collect(&mut b, "你好").await;
        assert!(
            evs.iter().any(|e| matches!(e, BrainEvent::Err(_))),
            "不可达端点应报错: {evs:?}"
        );
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "连接超时(0.8s)后应退出,实际 {}s",
            t0.elapsed().as_secs_f32()
        );
    }
}
