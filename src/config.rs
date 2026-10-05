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
    /// 是否启用 agent(false = 即使装了对应 CLI 也不用)
    pub enabled: bool,
    /// 适配协议: jcode(默认,常驻 repl)| one-shot(每轮新进程,DSH 等一次性 CLI)
    pub protocol: String,
    /// one-shot 协议的启动参数模板,{prompt} 会被替换为用户输入;
    /// 空则默认把输入作为唯一参数
    pub args: Vec<String>,
    /// 传给 `jcode repl -p <provider>` 的 provider;留空用 jcode 自动探测(仅 jcode 协议)
    pub provider: String,
    /// 工具白名单(逗号分隔,如 read,write,edit,bash);
    /// 空则用 --tool-profile minimal(只读工具集,仅 jcode 协议)
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
            protocol: "jcode".into(),
            args: Vec::new(),
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
        Self::load_from("config.toml")
    }

    /// 从指定路径加载(测试用注入路径,避免改进程 CWD 影响并行测试)。
    pub(crate) fn load_from(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let mut cfg = match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).context("解析 config.toml 失败")?,
            // 文件不存在 = 全新安装,用默认值;文件存在但读不了
            // (编码损坏/权限/占用)必须大声报错,否则静默回退默认配置
            // 会出现"明明填了 key 却提示未配置"这类难排查的问题。
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => {
                return Err(e).context(
                    "读取 config.toml 失败(文件需为 UTF-8 编码,勿用记事本另存为 ANSI/GBK)",
                );
            }
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
        assert_eq!(cfg.brain.agent.protocol, "jcode");
    }

    /// one-shot 协议配置(接 DeepSeek Harness 的 dsh --profile headless)。
    #[test]
    fn one_shot_agent_config_parses() {
        let text = r#"
            [brain]
            kind = "hybrid"

            [brain.agent]
            command = "dsh"
            protocol = "one-shot"
            args = ["--profile", "headless", "{prompt}"]
        "#;
        let cfg: Config = toml::from_str(text).expect("one-shot 配置应能解析");
        assert_eq!(cfg.brain.kind, "hybrid");
        assert_eq!(cfg.brain.agent.command, "dsh");
        assert_eq!(cfg.brain.agent.protocol, "one-shot");
        assert_eq!(
            cfg.brain.agent.args,
            vec!["--profile", "headless", "{prompt}"]
        );
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

    /// 回归: config.toml 存在但编码损坏(非 UTF-8)时必须报错,
    /// 不能静默回退默认配置(否则"填了 key 却提示未配置"极难排查)。
    #[test]
    fn corrupted_config_is_loud_error_not_silent_default() {
        let dir = std::env::temp_dir().join(format!("voxelf-cfg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        // GBK 编码的"配置"字节(非 UTF-8)
        let gbk: Vec<u8> = vec![0xC5, 0xE4, 0xD6, 0xC3, 0x0A];
        std::fs::write(&path, &gbk).unwrap();
        let r = Config::load_from(&path);
        std::fs::remove_dir_all(&dir).ok();
        assert!(r.is_err(), "损坏配置应报错而非静默回退默认: {r:?}");
        let msg = format!("{:#}", r.unwrap_err());
        assert!(msg.contains("config.toml"), "错误应指明配置文件: {msg}");
    }

    /// 回归: config.toml 不存在(全新安装)仍应回退默认值,不报错。
    #[test]
    fn missing_config_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("voxelf-cfg-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let r = Config::load_from(&path);
        std::fs::remove_dir_all(&dir).ok();
        assert!(r.is_ok(), "缺配置应回退默认: {r:?}");
        let cfg = r.unwrap();
        assert_eq!(cfg.brain.kind, "deepseek");
        assert!(cfg.deepseek.api_key.is_empty());
    }
}
