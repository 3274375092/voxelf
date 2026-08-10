pub mod agent;
pub mod deepseek;

use crate::config::Config;

/// 大脑输出的事件流,驱动动画与语音。
#[derive(Debug, Clone)]
pub enum BrainEvent {
    /// 增量文本(流式)
    Delta(String),
    /// 完整回复
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

    pub async fn run(&mut self, text: &str) -> Vec<BrainEvent> {
        match self {
            BrainKind::DeepSeek(b) => b.run(text).await,
            BrainKind::Agent(b) => b.run(text).await,
        }
    }
}
