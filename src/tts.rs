use anyhow::{Context, Result};
use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig, OfflineTtsModelConfig};

use crate::config::ModelCfg;

/// Kokoro TTS 封装。线程安全(单对象),内部 C 调用是阻塞的,调用方放 spawn_blocking。
pub struct Tts {
    engine: OfflineTts,
    voice_id: i32,
}

impl Tts {
    pub fn new(cfg: &ModelCfg) -> Result<Self> {
        let dir = cfg.tts_dir.as_os_str().to_string_lossy().into_owned();
        let mut kokoro = OfflineTtsKokoroModelConfig::default();
        kokoro.model = Some(format!("{dir}/{}", cfg.tts_model_file));
        kokoro.voices = Some(format!("{dir}/voices.bin"));
        kokoro.tokens = Some(format!("{dir}/tokens.txt"));
        kokoro.data_dir = Some(format!("{dir}/espeak-ng-data"));
        kokoro.dict_dir = Some(format!("{dir}/dict"));
        kokoro.lexicon = Some(format!("{dir}/lexicon-zh.txt"));

        let mut model = OfflineTtsModelConfig::default();
        model.kokoro = kokoro;
        model.num_threads = cfg.tts_threads;

        let mut config = OfflineTtsConfig::default();
        config.model = model;

        let engine = OfflineTts::create(&config).context("创建 TTS 引擎失败")?;
        let voice_id = match cfg.tts_voice.as_str() {
            "xiaobei" => 0,
            "xiaoni" => 1,
            "xiaoxiao" => 2,
            "xiaoyi" => 3,
            other => {
                tracing::warn!("未知音色 {other},使用默认 xiaoxiao");
                2
            }
        };
        tracing::info!("TTS 初始化完成: 采样率 {} 音色 {}", engine.sample_rate(), cfg.tts_voice);
        Ok(Self { engine, voice_id })
    }

    /// 合成一段语音,返回 (PCM f32, 采样率)。阻塞调用。
    pub fn synthesize(&self, text: &str) -> Option<(Vec<f32>, u32)> {
        let gen = GenerationConfig { sid: self.voice_id, speed: 1.0, ..Default::default() };
        let audio = self.engine.generate_with_config(text, &gen, None::<fn(&[f32], f32) -> bool>)?;
        let rate = self.engine.sample_rate();
        Some((audio.samples().to_vec(), rate as u32))
    }
}

/// 把 PCM f32 写成 16-bit 单声道 wav(测试用)。
pub fn write_wav(path: &str, samples: &[f32], sample_rate: u32) -> anyhow::Result<()> {
    use std::io::Write;
    let data_len = samples.len() * 2;
    let mut buf = Vec::with_capacity(44 + data_len);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    buf.extend_from_slice(b"WAVEfmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    buf.extend_from_slice(&2u16.to_le_bytes()); // block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(data_len as u32).to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        buf.extend_from_slice(&v.to_le_bytes());
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(&buf)?;
    Ok(())
}
