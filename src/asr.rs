use anyhow::{Context, Result};
use sherpa_onnx::{
    OnlineModelConfig, OnlineRecognizer, OnlineRecognizerConfig, OnlineStream,
    OnlineTransducerModelConfig, SileroVadModelConfig, VadModelConfig, VoiceActivityDetector,
    Wave,
};
use std::path::Path;
use std::thread;

use crate::audio::input::AudioChunk;
use crate::config::ModelCfg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsrEvent {
    /// VAD 检测到语音开始(开口瞬间,驱动聆听动画)
    SpeechStarted,
    /// 实时识别中间结果
    Partial(String),
    /// 一句话识别完成
    Final(String),
}

/// VAD 参数(可调,用于对照测试)
#[derive(Debug, Clone, Copy)]
pub struct VadParams {
    pub threshold: f32,
    pub min_silence: f32,
    pub min_speech: f32,
    pub max_speech: f32,
    /// 段弹出后额外追加的尾部音频时长(秒),弥补 VAD 对渐弱尾音的截断
    pub tail_pad: f32,
}

impl From<&ModelCfg> for VadParams {
    fn from(c: &ModelCfg) -> Self {
        Self {
            threshold: c.vad_threshold,
            min_silence: c.vad_min_silence,
            min_speech: c.vad_min_speech,
            max_speech: c.vad_max_speech,
            tail_pad: c.vad_tail_pad,
        }
    }
}

/// 流式 ASR:VAD 分段 + zipformer 流式识别。
/// sherpa-onnx 是同步 C 调用,整个对象只在一个专用线程中使用。
pub struct Asr {
    recognizer: OnlineRecognizer,
    vad: VoiceActivityDetector,
    sample_rate: i32,
    /// 最近 N 秒输入音频缓冲,用于段弹出后补偿 VAD 截掉的渐弱尾音
    tail_buf: std::collections::VecDeque<f32>,
    tail_pad: f32,
    /// VAD 上一帧的语音检测状态(检测上升沿发 SpeechStarted)
    last_detected: bool,
}

impl Asr {
    pub fn new(cfg: &ModelCfg) -> Result<Self> {
        Self::new_with_vad(cfg, VadParams::from(cfg))
    }

    pub fn new_with_vad(cfg: &ModelCfg, vad: VadParams) -> Result<Self> {
        let recognizer = create_recognizer(cfg)?;

        let detector = VoiceActivityDetector::create(
            &VadModelConfig {
                silero_vad: SileroVadModelConfig {
                    model: Some(cfg.vad_model.as_os_str().to_string_lossy().into_owned()),
                    threshold: vad.threshold,
                    min_silence_duration: vad.min_silence,
                    min_speech_duration: vad.min_speech,
                    window_size: 512,
                    max_speech_duration: vad.max_speech,
                },
                sample_rate: 16000,
                num_threads: 1,
                provider: None,
                debug: false,
                ten_vad: Default::default(),
            },
            30.0,
        )
        .context("创建 VAD 失败")?;

        tracing::info!(
            "ASR 初始化完成 (vad_threshold={} min_silence={})",
            vad.threshold,
            vad.min_silence
        );
        Ok(Self {
            recognizer,
            vad: detector,
            sample_rate: 16000,
            tail_buf: Default::default(),
            tail_pad: vad.tail_pad,
            last_detected: false,
        })
    }

    /// 喂入一段音频,返回产生的事件。samples 会被内部重采样到 16k(若设备采样率不同)。
    pub fn feed(&mut self, chunk: &AudioChunk) -> Vec<AsrEvent> {
        self.feed_inner(chunk).0
    }

    /// 同 feed,额外返回每个语音段的信息(时长秒,样本),诊断用。
    pub fn feed_inner(&mut self, chunk: &AudioChunk) -> (Vec<AsrEvent>, Vec<(f32, Vec<f32>)>) {
        let mut events = Vec::new();
        let mut segments = Vec::new();

        // 设备采样率 != 16k 时,先简单线性重采样到 16k 再进 VAD
        let samples: Vec<f32> = if chunk.sample_rate == self.sample_rate {
            chunk.samples.clone()
        } else {
            resample_linear(&chunk.samples, chunk.sample_rate, self.sample_rate)
        };

        self.vad.accept_waveform(&samples);

        // VAD 语音检测上升沿 → SpeechStarted(开口瞬间就通知 UI,不等 ASR 出字)
        let detected = self.vad.detected();
        if detected && !self.last_detected {
            events.push(AsrEvent::SpeechStarted);
        }
        self.last_detected = detected;

        // 维护尾部缓冲(供 VAD 段尾音补偿)
        self.tail_buf.extend(samples.iter().copied());
        let cap = (self.sample_rate as usize as f32 * self.tail_pad) as usize;
        while self.tail_buf.len() > cap {
            self.tail_buf.pop_front();
        }

        // 收集所有已完成的语音段
        loop {
            let mut seg: Vec<f32> = Vec::new();
            while let Some(s) = self.vad.front() {
                seg.extend_from_slice(s.samples());
                self.vad.pop();
            }
            if seg.is_empty() {
                break;
            }
            // 尾音补偿: 把段之后收到的音频(含渐弱尾音)也追加进去再识别
            let dur = seg.len() as f32 / self.sample_rate as f32;
            let mut recog_input = seg;
            recog_input.extend(self.tail_buf.iter().copied());
            events.extend(self.recognize_segment(&recog_input));
            segments.push((dur, recog_input));
        }
        (events, segments)
    }

    /// 对一段完整语音做流式识别,返回 Partial/Final 事件。
    fn recognize_segment(&mut self, seg: &[f32]) -> Vec<AsrEvent> {
        let t0 = std::time::Instant::now();
        let mut events = Vec::new();
        let stream = self.recognizer.create_stream();
        let mut last = String::new();

        // 每 100ms 喂一次并解码,产生中间结果
        for w in seg.chunks(1600) {
            stream.accept_waveform(self.sample_rate, w);
            self.decode_ready(&stream);
            if let Some(r) = self.recognizer.get_result(&stream)
                && !r.text.is_empty()
                && r.text != last
            {
                last = r.text.clone();
                events.push(AsrEvent::Partial(r.text));
            }
        }
        stream.input_finished();
        self.decode_ready(&stream);

        if let Some(r) = self.recognizer.get_result(&stream) {
            let text = r.text.trim().to_string();
            tracing::info!(
                "LATENCY ASR 段 {:.2}s 解码耗时 {:.2}s",
                seg.len() as f32 / self.sample_rate as f32,
                t0.elapsed().as_secs_f32()
            );
            if !text.is_empty() {
                if !events.is_empty() {
                    // 用最终结果顶掉最后的 Partial
                    events.pop();
                }
                events.push(AsrEvent::Final(text));
            }
        }
        events
    }

    fn decode_ready(&self, stream: &OnlineStream) {
        while self.recognizer.is_ready(stream) {
            self.recognizer.decode(stream);
        }
    }
}

/// 线性插值重采样(仅用于把设备采样率转到 16k;质量足够语音识别)。
pub(crate) fn resample_linear(input: &[f32], from: i32, to: i32) -> Vec<f32> {
    if input.is_empty() || from == to {
        return input.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let out_len = ((input.len() as f64) * ratio).ceil() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 / ratio;
        let idx = pos.floor() as usize;
        let frac = (pos - idx as f64) as f32;
        let a = input[idx.min(input.len() - 1)];
        let b = input[(idx + 1).min(input.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}

/// 创建在线识别器(不含 VAD),供 VAD 管线和直喂模式共用。
fn create_recognizer(cfg: &ModelCfg) -> Result<OnlineRecognizer> {
    let asr_dir = cfg.asr_dir.as_os_str().to_string_lossy().into_owned();
    let model = OnlineModelConfig {
        transducer: OnlineTransducerModelConfig {
            encoder: Some(format!("{asr_dir}/encoder.int8.onnx")),
            decoder: Some(format!("{asr_dir}/decoder.onnx")),
            joiner: Some(format!("{asr_dir}/joiner.int8.onnx")),
        },
        tokens: Some(format!("{asr_dir}/tokens.txt")),
        num_threads: cfg.asr_threads,
        debug: false,
        model_type: Some("zipformer2".into()),
        ..Default::default()
    };

    let config = OnlineRecognizerConfig {
        model_config: model,
        enable_endpoint: false,
        decoding_method: Some("greedy_search".into()),
        ..Default::default()
    };

    OnlineRecognizer::create(&config).context("创建 ASR 识别器失败")
}

/// 对一段 16k 音频直接跑识别(无 VAD 分段)。供诊断工具(diag)使用。
pub(crate) fn recognize_direct(cfg: &ModelCfg, audio: &[f32]) -> Result<String> {
    let recognizer = create_recognizer(cfg)?;
    let stream = recognizer.create_stream();
    for w in audio.chunks(1600) {
        stream.accept_waveform(16000, w);
        while recognizer.is_ready(&stream) {
            recognizer.decode(&stream);
        }
    }
    stream.input_finished();
    while recognizer.is_ready(&stream) {
        recognizer.decode(&stream);
    }
    Ok(recognizer
        .get_result(&stream)
        .map(|r| r.text.trim().to_string())
        .unwrap_or_default())
}

/// 在专用线程里跑 ASR 循环:接收麦克风 chunk,发事件。
pub fn spawn_asr_worker(
    cfg: ModelCfg,
    rx: flume::Receiver<AudioChunk>,
    tx: flume::Sender<AsrEvent>,
) -> Result<()> {
    thread::Builder::new()
        .name("asr".into())
        .spawn(move || {
            let mut asr = match Asr::new(&cfg) {
                Ok(a) => a,
                Err(e) => {
                    tracing::error!("ASR 初始化失败: {e:#}");
                    return;
                }
            };
            while let Ok(chunk) = rx.recv() {
                for ev in asr.feed(&chunk) {
                    let _ = tx.send(ev);
                }
            }
        })
        .context("启动 ASR 线程失败")?;
    Ok(())
}

/// 对 wav 文件跑一次识别(测试用),打印最终文本。
pub fn run_asr_file(cfg: &ModelCfg, path: &Path) -> Result<()> {
    let wave = Wave::read(path.to_str().unwrap_or_default()).context("读取 wav 失败")?;
    tracing::info!("wav: {} 采样率 {} 长度 {:.1}s", path.display(), wave.sample_rate(), wave.samples().len() as f32 / wave.sample_rate() as f32);
    let mut asr = Asr::new(cfg)?;
    let sr = wave.sample_rate();
    let mut text = String::new();
    for w in wave.samples().chunks(1600) {
        let chunk = AudioChunk { samples: w.to_vec(), sample_rate: sr };
        for ev in asr.feed(&chunk) {
            if let AsrEvent::Final(t) = ev {
                println!("识别结果: {t}");
                text = t;
            }
        }
    }
    // 冲刷 VAD 尾部缓冲(2s,确保最后一段收尾)
    for _ in 0..20 {
        for ev in asr.feed(&AudioChunk { samples: vec![0.0; 1600], sample_rate: 16000 }) {
            if let AsrEvent::Final(t) = ev {
                println!("识别结果: {t}");
                text = t;
            }
        }
    }
    if text.is_empty() {
        println!("(未识别到语音)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 重采样长度与值保真: 48k→16k 长度 1/3,信号形状不变(纯函数,无模型)。
    #[test]
    fn resample_linear_length_and_shape() {
        // 1kHz 正弦 @48k,1 秒
        let input: Vec<f32> = (0..48000)
            .map(|i| (i as f32 * 2.0 * std::f32::consts::PI * 1000.0 / 48000.0).sin())
            .collect();
        let out = resample_linear(&input, 48000, 16000);
        assert_eq!(out.len(), 16000, "48k→16k 长度应为 1/3");
        // 同采样率直通
        assert_eq!(resample_linear(&input, 48000, 48000).len(), 48000);
        // 空输入
        assert!(resample_linear(&[], 48000, 16000).is_empty());
        // 形状校验: 48k→16k 整除,采样点严格对齐,16k 输出应逐点接近 sin(2π·i/16)
        for (i, &v) in out.iter().enumerate().take(1000).skip(1) {
            let expect = (2.0 * std::f32::consts::PI * i as f32 / 16.0).sin();
            assert!((v - expect).abs() < 1e-3, "i={i}: {v} vs {expect}");
        }
    }

    /// 复现测试: 开口瞬间必须有 SpeechStarted 事件,且先于 Partial。
    /// 若缺失,说明 UI 动画在开口到 ASR 出字之间没有反馈(状态不同步)。
    #[test]
    fn speech_started_fires_before_partial() {
        let cfg = crate::config::ModelCfg::default();
        let mut asr = match Asr::new(&cfg) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("SKIP: ASR 初始化失败(模型缺失?): {e}");
                return;
            }
        };
        let wave = match Wave::read("assets/models/asr-zh/test.wav") {
            Some(w) => w,
            None => {
                eprintln!("SKIP: test.wav 缺失");
                return;
            }
        };
        let sr = wave.sample_rate();
        let mut events: Vec<AsrEvent> = Vec::new();
        for w in wave.samples().chunks(1600) {
            let chunk = AudioChunk { samples: w.to_vec(), sample_rate: sr };
            events.extend(asr.feed(&chunk));
        }
        eprintln!("事件序列({}): {:?}", events.len(), events);
        let started = events.iter().position(|e| matches!(e, AsrEvent::SpeechStarted));
        let partial = events.iter().position(|e| matches!(e, AsrEvent::Partial(_)));
        assert!(started.is_some(), "语音段应产生 SpeechStarted: {events:?}");
        assert!(partial.is_some(), "语音段应产生 Partial: {events:?}");
        assert!(
            started.unwrap() < partial.unwrap(),
            "SpeechStarted 应先于 Partial(开口即聆听): {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(e, AsrEvent::Final(_))),
            "应产生 Final: {events:?}"
        );
    }

    /// 复现测试: 纯静音不应触发 SpeechStarted(避免误聆听)。
    #[test]
    fn silence_does_not_fire_speech_started() {
        let cfg = crate::config::ModelCfg::default();
        let mut asr = match Asr::new(&cfg) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("SKIP: ASR 初始化失败: {e}");
                return;
            }
        };
        let silence = vec![0.0f32; 16000 * 2]; // 2 秒静音
        let mut events = Vec::new();
        for w in silence.chunks(1600) {
            let chunk = AudioChunk { samples: w.to_vec(), sample_rate: 16000 };
            events.extend(asr.feed(&chunk));
        }
        assert!(
            !events.iter().any(|e| matches!(e, AsrEvent::SpeechStarted)),
            "静音不应触发 SpeechStarted: {events:?}"
        );
    }
}
