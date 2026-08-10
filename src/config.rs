use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub deepseek: DeepSeekCfg,
    pub brain: BrainCfg,
    pub models: ModelCfg,
    pub ui: UiCfg,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DeepSeekCfg {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BrainCfg {
    pub kind: String,
    pub agent: AgentCfg,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AgentCfg {
    pub command: String,
    pub workdir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ModelCfg {
    pub asr_dir: PathBuf,
    pub vad_model: PathBuf,
    pub tts_dir: PathBuf,
    pub tts_model_file: String,
    pub tts_voice: String,
    pub asr_threads: i32,
    pub tts_threads: i32,
    /// VAD 语音/静音判定阈值(0-1),越低越不容易吞句尾
    pub vad_threshold: f32,
    /// VAD 判定"一句话说完"所需的尾部静音秒数,越大越不易截断句尾
    pub vad_min_silence: f32,
    /// VAD 判定"开始说话"所需的最小语音时长(秒)
    pub vad_min_speech: f32,
    /// VAD 允许的单段最长语音(秒)
    pub vad_max_speech: f32,
    /// 段弹出后追加的尾部音频时长(秒),补偿渐弱尾音被 VAD 截断
    pub vad_tail_pad: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UiCfg {
    pub window_width: i32,
    pub window_height: i32,
}

impl Default for DeepSeekCfg {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.deepseek.com".into(),
            model: "deepseek-chat".into(),
            system_prompt: "你是一个住在像素世界里的可爱小精灵伙伴,名字叫 Vox。用简短、口语化的中文回复,每次不超过 3 句话。回复会被语音合成朗读,不要使用列表或符号。".into(),
        }
    }
}
impl Default for AgentCfg {
    fn default() -> Self {
        Self { command: "jcode".into(), workdir: ".".into() }
    }
}
impl Default for BrainCfg {
    fn default() -> Self {
        Self { kind: "deepseek".into(), agent: AgentCfg::default() }
    }
}
impl Default for ModelCfg {
    fn default() -> Self {
        Self {
            asr_dir: "assets/models/asr-zh".into(),
            vad_model: "assets/models/vad/silero_vad.onnx".into(),
            tts_dir: "assets/models/kokoro".into(),
            tts_model_file: "model.int8.onnx".into(),
            tts_voice: "xiaoxiao".into(),
            asr_threads: 2,
            tts_threads: 2,
            vad_threshold: 0.3,
            vad_min_silence: 0.8,
            vad_min_speech: 0.25,
            vad_max_speech: 20.0,
            vad_tail_pad: 0.6,
        }
    }
}
impl Default for UiCfg {
    fn default() -> Self {
        Self { window_width: 960, window_height: 600 }
    }
}
impl Default for Config {
    fn default() -> Self {
        Self {
            deepseek: DeepSeekCfg::default(),
            brain: BrainCfg::default(),
            models: ModelCfg::default(),
            ui: UiCfg::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let mut cfg = match std::fs::read_to_string("config.toml") {
            Ok(text) => toml::from_str(&text).context("解析 config.toml 失败")?,
            Err(_) => Config::default(),
        };
        if cfg.deepseek.api_key.trim().is_empty() {
            if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
                cfg.deepseek.api_key = key;
            }
        }
        Ok(cfg)
    }
}
