use anyhow::{Context, Result};
use sherpa_onnx::{
    OnlineModelConfig, OnlineRecognizer, OnlineRecognizerConfig, OnlineStream,
    OnlineTransducerModelConfig, SileroVadModelConfig, VadModelConfig, VoiceActivityDetector,
    Wave,
};
use std::path::Path;
use std::thread;

use crate::audio::input::AudioChunk;
use crate::config::{Config, ModelCfg};

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

/// 创建在线识别器(不含 VAD),供 VAD 管线和直喂模式共用。
fn create_recognizer(cfg: &ModelCfg) -> Result<OnlineRecognizer> {
    let asr_dir = cfg.asr_dir.as_os_str().to_string_lossy().into_owned();
    let mut model = OnlineModelConfig::default();
    model.transducer = OnlineTransducerModelConfig {
        encoder: Some(format!("{asr_dir}/encoder.int8.onnx")),
        decoder: Some(format!("{asr_dir}/decoder.onnx")),
        joiner: Some(format!("{asr_dir}/joiner.int8.onnx")),
    };
    model.tokens = Some(format!("{asr_dir}/tokens.txt"));
    model.num_threads = cfg.asr_threads;
    model.debug = false;
    model.model_type = Some("zipformer2".into());

    let mut config = OnlineRecognizerConfig::default();
    config.model_config = model;
    config.enable_endpoint = false;
    config.decoding_method = Some("greedy_search".into());

    OnlineRecognizer::create(&config).context("创建 ASR 识别器失败")
}

/// 对一段 16k 音频直接跑识别(无 VAD 分段)。
fn recognize_direct(cfg: &ModelCfg, audio: &[f32]) -> Result<String> {
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

/// 识别结果与目标文本的差异描述(定位尾部截断用)。
fn diff_line(rec: &str, target: &str) -> String {
    if rec == target {
        return "完全一致".into();
    }
    let r: Vec<char> = rec.chars().collect();
    let t: Vec<char> = target.chars().collect();
    let mut i = 0;
    while i < r.len() && i < t.len() && r[i] == t[i] {
        i += 1;
    }
    let tail_r: String = r.iter().skip(i).collect();
    let tail_t: String = t.iter().skip(i).collect();
    format!("前{i}字一致; 识别尾=[{tail_r}] 目标尾=[{tail_t}]")
}

/// ASR 定位测试: 用 TTS 合成已知文本(ground truth),跑三种模式对照:
/// A 当前 VAD 配置 | B 无 VAD 直喂 | C 宽松 VAD(阈值 0.3 / 尾静音 1s)。
/// 用于定位"吞句尾"是 VAD 截断还是识别器本身的问题。
pub fn run_asr_diag(cfg: &Config, text: &str) -> Result<()> {
    let tts = crate::tts::Tts::new(&cfg.models)?;
    let (samples, rate) = tts.synthesize(text).context("TTS 合成失败")?;
    // TTS 是 24k,统一重采样到 16k(与麦克风管线一致)
    let samples = resample_linear(&samples, rate as i32, 16000);
    let rate = 16000;

    // 前后补 0.5s 静音,模拟真实麦克风环境
    let pad = (rate / 2) as usize;
    let mut audio = vec![0f32; pad];
    audio.extend_from_slice(&samples);
    audio.extend_from_slice(&vec![0f32; pad]);

    // 真实语音末尾: 能量 > 0.01 的最后一个采样
    let true_end = audio
        .iter()
        .rposition(|&s| s.abs() > 0.01)
        .map(|i| i as f32 / rate as f32)
        .unwrap_or(0.0);

    println!("=================== ASR 定位测试 ===================");
    println!("目标文本 : {text}");
    println!("音频时长 : {:.2}s  真实语音末尾: {:.3}s", audio.len() as f32 / rate as f32, true_end);

    let base = VadParams::from(&cfg.models);
    let (segs_a, result_a, seg_audio_a) = recognize_with_vad(cfg, &audio, base)?;
    report("A 当前VAD", &base, &segs_a, &result_a, text);

    // 模式 D: 直接把 A 捕获的 VAD 段喂给识别器(无 VAD),判断段本身是否完整
    if let Some(seg) = seg_audio_a.first() {
        let result_d = recognize_direct(&cfg.models, seg)?;
        println!("--------------------------------------------------");
        println!("D VAD段直喂(段长 {:.2}s)", seg.len() as f32 / 16000.0);
        println!("   识别    : {result_d}");
        println!("   与目标 : {}", diff_line(&result_d, text));
        crate::tts::write_wav("diag_seg.wav", seg, 16000)?;
        println!("   段音频已保存 diag_seg.wav");
    }

    let result_b = recognize_direct(&cfg.models, &audio)?;
    println!("--------------------------------------------------");
    println!("B 无VAD直喂");
    println!("   识别    : {result_b}");
    println!("   与目标 : {}", diff_line(&result_b, text));

    let relaxed = VadParams { threshold: 0.3, min_silence: 1.0, min_speech: 0.25, max_speech: 20.0, tail_pad: 0.8 };
    let (segs_c, result_c, _) = recognize_with_vad(cfg, &audio, relaxed)?;
    report("C 宽松VAD", &relaxed, &segs_c, &result_c, text);
    Ok(())
}

fn recognize_with_vad(cfg: &Config, audio: &[f32], vad: VadParams) -> Result<(Vec<f32>, String, Vec<Vec<f32>>)> {
    let mut asr = Asr::new_with_vad(&cfg.models, vad)?;
    let mut segments = Vec::new();
    let mut finals = Vec::new();
    let mut seg_dumps: Vec<Vec<f32>> = Vec::new();
    let mut asr_events: Vec<AsrEvent> = Vec::new();
    for w in audio.chunks(1600) {
        let (events, segs) = asr.feed_inner(&AudioChunk { samples: w.to_vec(), sample_rate: 16000 });
        for (dur, samples) in segs {
            segments.push(dur);
            seg_dumps.push(samples);
        }
        asr_events.extend(events);
    }
    // 冲刷 VAD 尾部缓冲(至少 2s,确保任何配置下段都能收尾)
    for _ in 0..20 {
        let (events, segs) = asr.feed_inner(&AudioChunk { samples: vec![0.0; 1600], sample_rate: 16000 });
        for (dur, samples) in segs {
            segments.push(dur);
            seg_dumps.push(samples);
        }
        asr_events.extend(events);
    }
    for ev in asr_events {
        if let AsrEvent::Final(t) = ev {
            finals.push(t);
        }
    }
    // 输出每个 VAD 段的内容统计(诊断尾部截断)
    for (i, seg) in seg_dumps.iter().enumerate() {
        let end = seg
            .iter()
            .rposition(|&s| s.abs() > 0.01)
            .map(|j| j as f32 / 16000.0)
            .unwrap_or(0.0);
        println!("   VAD段{i}内容: 长度 {:.2}s, 段内末尾能量>0.01 在 {:.3}s", seg.len() as f32 / 16000.0, end);
    }
    Ok((segments, finals.join(" "), seg_dumps))
}

fn report(tag: &str, vad: &VadParams, segments: &[f32], result: &str, target: &str) {
    println!("--------------------------------------------------");
    println!("{tag} (threshold={} min_silence={})", vad.threshold, vad.min_silence);
    for (i, s) in segments.iter().enumerate() {
        println!("   段{i}: {:.2}s", s);
    }
    println!("   识别    : {result}");
    println!("   与目标 : {}", diff_line(result, target));
}

/// 全链路离线延迟测试: TTS 合成已知文本 → 按真实时间节奏喂给 ASR → 测各阶段耗时。
/// 输出用户最关心的数字: "说完话后多久出识别结果"(含 VAD 静音判定 + 解码)。
pub fn run_latency_test(cfg: &Config, text: &str) -> Result<()> {
    use std::time::Instant;
    println!("========== 延迟定位测试 ==========");
    println!("目标: {text}");

    // 1. TTS
    let tts = crate::tts::Tts::new(&cfg.models)?;
    let t0 = Instant::now();
    let (samples, rate) = tts.synthesize(text).context("TTS 合成失败")?;
    let tts_s = t0.elapsed().as_secs_f32();
    println!(
        "[TTS] 合成耗时 {tts_s:.2}s, 音频 {:.1}s",
        samples.len() as f32 / rate as f32
    );

    // 2. ASR: 模拟实时麦克风(100ms 块 + 真实时间间隔),前后补 0.3s 静音
    let samples16 = resample_linear(&samples, rate as i32, 16000);
    let pad = 4800; // 0.3s @ 16k
    let mut audio = vec![0f32; pad];
    audio.extend_from_slice(&samples16);
    audio.extend_from_slice(&vec![0f32; pad]);
    let true_end = audio
        .iter()
        .rposition(|&s| s.abs() > 0.01)
        .map(|i| i as f32 / 16000.0)
        .unwrap_or(0.0);

    let mut asr = Asr::new(&cfg.models)?;
    let feed_start = Instant::now();
    let mut final_at: Option<Instant> = None;
    let mut final_text = String::new();
    for w in audio.chunks(1600) {
        for ev in asr.feed(&AudioChunk { samples: w.to_vec(), sample_rate: 16000 }) {
            if let AsrEvent::Final(t) = ev {
                final_at = Some(Instant::now());
                final_text = t;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100)); // 实时节奏
    }
    // 说完后继续等 VAD 收尾(最多 6s)
    let mut waited = 0.0f32;
    while final_at.is_none() && waited < 6.0 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        waited += 0.1;
        for ev in asr.feed(&AudioChunk { samples: vec![0.0; 1600], sample_rate: 16000 }) {
            if let AsrEvent::Final(t) = ev {
                final_at = Some(Instant::now());
                final_text = t;
            }
        }
    }
    match final_at {
        Some(ft) => {
            let total = ft.duration_since(feed_start).as_secs_f32();
            let after_speech = total - true_end;
            println!("[ASR] 识别结果: {final_text}");
            println!(
                "[ASR] 说完后 {after_speech:.2}s 出结果 (语音末尾 {true_end:.2}s; 含 VAD 静音判定 + 解码)"
            );
        }
        None => println!("[ASR] 6s 内未出结果!"),
    }
    Ok(())
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
        let sr = wave.sample_rate() as i32;
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
