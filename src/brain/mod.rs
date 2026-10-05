pub mod adapters;
pub mod agent;
pub mod deepseek;
pub mod hybrid;

use crate::config::Config;

/// 大脑输出的事件流,驱动动画、气泡与语音。
#[derive(Debug, Clone)]
pub enum BrainEvent {
    /// 增量文本(流式)
    Delta(String),
    /// 完整回复(流结束)
    Done(String),
    Err(String),
    /// agent 正在执行工具(携带工具名,驱动"干活"动画)
    Working(String),
}

/// 大脑实现:DeepSeek API、agent 子进程(常驻 repl),或双层 hybrid(闲聊 DeepSeek + 操作 agent)。
pub enum BrainKind {
    DeepSeek(deepseek::DeepSeekBrain),
    Agent(agent::AgentBrain),
    Hybrid(hybrid::HybridBrain),
}

impl BrainKind {
    pub fn from_config(cfg: &Config) -> Self {
        match cfg.brain.kind.as_str() {
            "agent" => BrainKind::Agent(agent::AgentBrain::new(&cfg.brain.agent)),
            "hybrid" => BrainKind::Hybrid(hybrid::HybridBrain::new(cfg)),
            _ => BrainKind::DeepSeek(deepseek::DeepSeekBrain::new(&cfg.deepseek)),
        }
    }

    /// 流式运行: 增量/完成/错误事件实时推入 tx。返回即流结束。
    pub async fn run_streaming(&mut self, text: &str, tx: flume::Sender<BrainEvent>) {
        match self {
            BrainKind::DeepSeek(b) => b.run_streaming(text, tx).await,
            BrainKind::Agent(b) => b.run_streaming(text, tx).await,
            BrainKind::Hybrid(b) => b.run_streaming(text, tx).await,
        }
    }

    /// 收集式运行(CLI 等一次性场景)。
    pub async fn run(&mut self, text: &str) -> Vec<BrainEvent> {
        let (tx, rx) = flume::unbounded();
        self.run_streaming(text, tx).await;
        rx.drain().collect()
    }
}
