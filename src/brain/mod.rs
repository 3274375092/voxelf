pub mod agent;
pub mod deepseek;

use crate::config::Config;

/// 大脑输出的事件流,驱动动画、气泡与语音。
#[derive(Debug, Clone)]
pub enum BrainEvent {
    /// 增量文本(流式)
    Delta(String),
    /// 完整回复(流结束)
    Done(String),
    Err(String),
}

/// 大脑实现:DeepSeek API 或 agent 子进程。
/// 后续加 pi / MCP 直连时在这里扩展。
pub enum BrainKind {
    DeepSeek(deepseek::DeepSeekBrain),
    Agent(agent::AgentBrain),
}

impl BrainKind {
    pub fn from_config(cfg: &Config) -> Self {
        match cfg.brain.kind.as_str() {
            "agent" => BrainKind::Agent(agent::AgentBrain::new(&cfg.brain.agent)),
            _ => BrainKind::DeepSeek(deepseek::DeepSeekBrain::new(&cfg.deepseek)),
        }
    }

    /// 流式运行: 增量/完成/错误事件实时推入 tx。返回即流结束。
    pub async fn run_streaming(&mut self, text: &str, tx: flume::Sender<BrainEvent>) {
        match self {
            BrainKind::DeepSeek(b) => b.run_streaming(text, tx).await,
            BrainKind::Agent(b) => b.run_streaming(text, tx).await,
        }
    }

    /// 收集式运行(CLI 等一次性场景)。
    pub async fn run(&mut self, text: &str) -> Vec<BrainEvent> {
        let (tx, rx) = flume::unbounded();
        self.run_streaming(text, tx).await;
        rx.drain().collect()
    }
}
