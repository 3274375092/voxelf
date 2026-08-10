use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Listening,
    Thinking,
    Speaking,
    Working,
    Error,
}

/// 所有线程共享的 UI 状态。由音频/ASR/大脑/TTS 线程写入,UI 每帧读取。
#[derive(Debug, Clone, Default)]
pub struct UiState {
    pub phase: Phase,
    /// ASR 实时识别中间结果
    pub asr_partial: String,
    /// 用户最近一句完整输入
    pub last_user_text: String,
    /// 正在播报的回复文本
    pub reply_text: String,
    /// 麦克风电平 0..1
    pub mic_level: f32,
    /// 状态提示(如加载失败信息)
    pub status: String,
    pub error: Option<String>,
}

pub type SharedState = Arc<Mutex<UiState>>;
