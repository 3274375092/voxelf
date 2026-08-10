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
    /// 实时识别中间结果
    Partial(String),
    /// 一句话识别完成
    Final(String),
}

/// 流式 ASR:VAD 分段 + zipformer 流式识别。
/// sherpa-onnx 是同步 C 调用,整个对象只在一个专用线程中使用。
pub struct Asr {
    recognizer: OnlineRecognizer,
    vad: VoiceActivityDetector,
    sample_rate: i32,
}

impl Asr {
    pub fn new(cfg: &ModelCfg) -> Result<Self> {
        let asr_dir = cfg.asr_dir.as_os_str().to_string_lossy().into_owned();
        let mut model = OnlineModelConfig::default();
        model.transducer = OnlineTransducerModelConfig {
            encoder: Some(format!("{asr_dir}/encoder-epoch-99-avg-1.int8.onnx")),
            decoder: Some(format!("{asr_dir}/decoder-epoch-99-avg-1.int8.onnx")),
            joiner: Some(format!("{asr_dir}/joiner-epoch-99-avg-1.int8.onnx")),
        };
        model.tokens = Some(format!("{asr_dir}/tokens.txt"));
        model.num_threads = cfg.asr_threads;
        model.debug = false;
        model.model_type = Some("zipformer2".into());

        let mut config = OnlineRecognizerConfig::default();
        config.model_config = model;
        config.enable_endpoint = false;
        config.decoding_method = Some("greedy_search".into());

        let recognizer = OnlineRecognizer::create(&config).context("创建 ASR 识别器失败")?;

        let vad = VoiceActivityDetector::create(
            &VadModelConfig {
                silero_vad: SileroVadModelConfig {
                    model: Some(cfg.vad_model.as_os_str().to_string_lossy().into_owned()),
                    threshold: 0.5,
                    min_silence_duration: 0.5,
                    min_speech_duration: 0.25,
                    window_size: 512,
                    max_speech_duration: 20.0,
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

        tracing::info!("ASR 初始化完成");
        Ok(Self { recognizer, vad, sample_rate: 16000 })
    }

    /// 喂入一段音频,返回产生的事件。samples 会被内部重采样到 16k(若设备采样率不同)。
    pub fn feed(&mut self, chunk: &AudioChunk) -> Vec<AsrEvent> {
        let mut events = Vec::new();

        // 设备采样率 != 16k 时,先简单线性重采样到 16k 再进 VAD
        let samples: Vec<f32> = if chunk.sample_rate == self.sample_rate {
            chunk.samples.clone()
        } else {
            resample_linear(&chunk.samples, chunk.sample_rate, self.sample_rate)
        };

        self.vad.accept_waveform(&samples);

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
            events.extend(self.recognize_segment(&seg));
        }
        events
    }

    /// 对一段完整语音做流式识别,返回 Partial/Final 事件。
    fn recognize_segment(&mut self, seg: &[f32]) -> Vec<AsrEvent> {
        let mut events = Vec::new();
        let stream = self.recognizer.create_stream();
        let mut last = String::new();

        // 每 100ms 喂一次并解码,产生中间结果
        for w in seg.chunks(1600) {
            stream.accept_waveform(self.sample_rate, w);
            self.decode_ready(&stream);
            if let Some(r) = self.recognizer.get_result(&stream) {
                if !r.text.is_empty() && r.text != last {
                    last = r.text.clone();
                    events.push(AsrEvent::Partial(r.text));
                }
            }
        }
        stream.input_finished();
        self.decode_ready(&stream);

        if let Some(r) = self.recognizer.get_result(&stream) {
            let text = r.text.trim().to_string();
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
fn resample_linear(input: &[f32], from: i32, to: i32) -> Vec<f32> {
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
    let sr = wave.sample_rate() as i32;
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
    // 处理缓冲中残留
    for _ in 0..5 {
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
