//! ASR 诊断工具(CLI 命令): 定位"吞句尾"与延迟问题。
//! 与 asr.rs(识别器核心)分离,避免诊断代码混入生产路径。

use anyhow::{Context, Result};

use crate::asr::{recognize_direct, resample_linear, Asr, AsrEvent, VadParams};
use crate::audio::input::AudioChunk;
use crate::config::Config;

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
