use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Listening,
    Thinking,
    Speaking,
    Working,
    Error,
}

impl Phase {
    pub fn label(&self) -> &'static str {
        match self {
            Phase::Idle => "待机",
            Phase::Listening => "聆听",
            Phase::Thinking => "思考",
            Phase::Speaking => "说话",
            Phase::Working => "干活",
            Phase::Error => "出错",
        }
    }
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

impl Default for Phase {
    fn default() -> Self {
        Phase::Idle
    }
}

pub type SharedState = Arc<Mutex<UiState>>;

pub fn set_phase(state: &SharedState, phase: Phase) {
    if let Ok(mut s) = state.lock() {
        s.phase = phase;
    }
}
