use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub deepseek: DeepSeekCfg,
    pub brain: BrainCfg,
    pub models: ModelCfg,
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
    /// 是否启用 agent(false = 即使装了 jcode 也不用)
    pub enabled: bool,
    /// 传给 `jcode repl -p <provider>` 的 provider;留空用 jcode 自动探测
    pub provider: String,
    /// 工具白名单(逗号分隔,如 read,write,edit,bash);
    /// 空则用 --tool-profile minimal(只读工具集)
    pub tools: String,
    /// 单轮 agent 响应超时(秒),超时后杀掉进程并报错
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ModelCfg {
    pub asr_dir: PathBuf,
    pub vad_model: PathBuf,
    pub tts_dir: PathBuf,
    pub tts_model_file: String,
    pub tts_voice: String,
    /// kokoro | vits(vits-zh 更快但音质略降)
    pub tts_kind: String,
    /// ASR 模型架构: zipformer2(2024+ 中文模型,默认) | zipformer(2023 中英双语模型)
    pub asr_model_type: String,
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

impl Default for DeepSeekCfg {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.deepseek.com".into(),
            model: "deepseek-chat".into(),
            system_prompt: "你是一个住在像素世界里的可爱小精灵伙伴,名字叫 Vox。用简短、口语化的中文回复,每次不超过 3 句话。语气活泼、温暖、有点俏皮。回复会被语音合成朗读,不要使用列表或符号。句子之间用句号或感叹号分隔,方便语音流式播放。".into(),
        }
    }
}
impl Default for AgentCfg {
    fn default() -> Self {
        Self {
            command: "jcode".into(),
            workdir: ".".into(),
            enabled: true,
            provider: "deepseek".into(),
            tools: String::new(),
            timeout_secs: 120,
        }
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
            tts_dir: "assets/models/vits-zh".into(),
            tts_model_file: "model.onnx".into(),
            tts_voice: "0".into(),
            tts_kind: "vits".into(),
            asr_model_type: "zipformer2".into(),
            asr_threads: 2,
            tts_threads: 4,
            vad_threshold: 0.3,
            vad_min_silence: 0.5,
            vad_min_speech: 0.25,
            vad_max_speech: 20.0,
            vad_tail_pad: 0.6,
        }
    }
}
impl Config {
    pub fn load() -> Result<Self> {
        let mut cfg = match std::fs::read_to_string("config.toml") {
            Ok(text) => toml::from_str(&text).context("解析 config.toml 失败")?,
            Err(_) => Config::default(),
        };
        if cfg.deepseek.api_key.trim().is_empty()
            && let Ok(key) = std::env::var("DEEPSEEK_API_KEY")
        {
            cfg.deepseek.api_key = key;
        }
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// config.example.toml 必须能被当前结构解析,防止示例与代码漂移
    /// (示例里写了的字段解析不出来 = 用户照抄就报错)。
    #[test]
    fn example_config_parses() {
        let text = std::fs::read_to_string("config.example.toml")
            .expect("config.example.toml 应存在");
        let cfg: Config = toml::from_str(&text).expect("示例配置应能被解析");
        assert_eq!(cfg.brain.kind, "deepseek");
        assert_eq!(cfg.models.tts_kind, "vits");
        assert_eq!(cfg.models.vad_min_silence, 0.5);
        assert_eq!(cfg.brain.agent.command, "jcode");
    }

    /// 未知字段(如旧版本遗留的 [ui] 段)应被忽略,不能导致启动失败。
    #[test]
    fn unknown_fields_are_ignored() {
        let text = r#"
            [ui]
            window_width = 960
            window_height = 600
        "#;
        let cfg: Config = toml::from_str(text).expect("未知字段应被忽略");
        assert_eq!(cfg.brain.kind, "deepseek");
    }

    /// 默认值兜底: 缺失字段全部走 Default(与示例一致的 VAD 默认)。
    #[test]
    fn defaults_fill_missing_fields() {
        let cfg = Config::default();
        assert_eq!(cfg.models.asr_dir, PathBuf::from("assets/models/asr-zh"));
        assert_eq!(cfg.brain.agent.timeout_secs, 120);
        assert!(cfg.deepseek.base_url.contains("deepseek"));
    }
}
