use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::state::{Phase, SharedState, UiState};

/// 一帧麦克风音频(约 100ms)。
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub samples: Vec<f32>,
    pub sample_rate: i32,
}

/// 启动麦克风采集线程。音频回调里只做:下混单声道、算电平、发 channel。
/// 回调线程必须保持存活,`cpal::Stream` 不能被 drop。
pub fn spawn_mic(tx: flume::Sender<AudioChunk>, state: SharedState) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("没有找到默认麦克风")?;
    let supported = device
        .default_input_config()
        .context("获取麦克风默认配置失败")?;
    let config = supported.config();
    let sample_format = supported.sample_format();
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate.0 as i32;

    tracing::info!(
        "麦克风: {:?} 格式 {:?} 声道 {} 采样率 {}",
        device.name().unwrap_or_default(),
        sample_format,
        channels,
        sample_rate
    );

    let err_fn = |e| tracing::error!("音频流错误: {e:?}");

    let tx_send = tx.clone();
    let state_cb = state.clone();
    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _| {
                push_chunk(&tx_send, &state_cb, data, channels, sample_rate);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _| {
                let buf: Vec<f32> = data
                    .chunks(channels)
                    .map(|f| f.iter().map(|&s| s as f32 / 32768.0).sum::<f32>() / channels as f32)
                    .collect();
                push_chunk(&tx_send, &state_cb, &buf, 1, sample_rate);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _| {
                let buf: Vec<f32> = data
                    .chunks(channels)
                    .map(|f| {
                        f.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).sum::<f32>()
                            / channels as f32
                    })
                    .collect();
                push_chunk(&tx_send, &state_cb, &buf, 1, sample_rate);
            },
            err_fn,
            None,
        ),
        other => anyhow::bail!("不支持的采样格式: {other:?}"),
    }
    .context("创建麦克风输入流失败")?;

    stream.play().context("启动麦克风失败")?;

    // stream 必须存活:放进线程里 park 到进程结束。
    thread::Builder::new()
        .name("mic-keepalive".into())
        .spawn(move || loop {
            thread::park();
        })
        .context("启动麦克风线程失败")?;
    // 把 stream 挪进线程,避免主线程 drop。
    thread::Builder::new()
        .name("mic-holder".into())
        .spawn(move || {
            let _stream = stream;
            loop {
                thread::park();
            }
        })
        .context("启动麦克风持有线程失败")?;
    Ok(())
}

fn push_chunk(tx: &flume::Sender<AudioChunk>, state: &SharedState, data: &[f32], channels: usize, rate: i32) {
    if data.is_empty() {
        return;
    }
    let mono: Vec<f32> = if channels == 1 {
        data.to_vec()
    } else {
        data.chunks(channels)
            .map(|f| f.iter().sum::<f32>() / channels as f32)
            .collect()
    };
    // 电平: RMS,映射到 0..1
    let rms = (mono.iter().map(|s| s * s).sum::<f32>() / mono.len() as f32).sqrt();
    if let Ok(mut s) = state.lock() {
        s.mic_level = (rms * 6.0).min(1.0).max(s.mic_level * 0.5);
        if s.mic_level < 0.02 {
            s.mic_level = 0.0;
        }
        // 有明显人声时进入聆听状态(由 ASR 确认后细化)
        if s.mic_level > 0.12 && s.phase == Phase::Idle {
            s.phase = Phase::Listening;
        }
    }
    // 打包成 100ms 一帧(48k = 4800, 44.1k = 4410, 16k = 1600)
    const TARGET_MS: usize = 100;
    const MAX_CHUNK: usize = 4800;
    let target = (rate as usize * TARGET_MS / 1000).clamp(160, MAX_CHUNK);
    for c in mono.chunks(target) {
        if c.len() >= 160 {
            let _ = tx.send(AudioChunk { samples: c.to_vec(), sample_rate: rate });
        }
    }
}

// 保持 UiState 引用,避免未使用告警
#[allow(dead_code)]
fn _touch(_s: &UiState) {}
