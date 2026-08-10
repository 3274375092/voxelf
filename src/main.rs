mod audio;
mod asr;
mod brain;
mod config;
mod state;
mod tts;
mod ui;

use config::Config;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use audio::output::Player;
use brain::{BrainEvent, BrainKind};
use state::{Phase, SharedState, UiState};

#[derive(Parser)]
#[command(name = "voxelf", version, about = "语音交互像素伙伴: 麦克风 -> DeepSeek -> 语音回复,像素小人动画呈现")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// 图形界面主程序(默认)
    Run,
    /// 渲染一帧并保存 PNG 后退出(测试用)
    Smoke { out: Option<String> },
    /// 对 wav 文件跑语音识别
    AsrFile { wav: String },
    /// 文本合成语音,保存为 wav
    Tts { text: String, out: Option<String> },
    /// 纯文本对话(不走语音)
    Chat { text: String },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let cfg = config::Config::load()?;

    match cli.cmd.unwrap_or(Cmd::Run) {
        Cmd::Run => run_app(cfg),
        Cmd::Smoke { out } => {
            let path = out.unwrap_or_else(|| "smoke.png".to_string());
            ui::app::run_smoke(cfg, &path);
            Ok(())
        }
        Cmd::AsrFile { wav } => asr::run_asr_file(&cfg.models, Path::new(&wav)),
        Cmd::Tts { text, out } => {
            let engine = tts::Tts::new(&cfg.models)?;
            let (samples, rate) = engine
                .synthesize(&text)
                .context("语音合成失败")?;
            let path = out.unwrap_or_else(|| "tts_out.wav".to_string());
            tts::write_wav(&path, &samples, rate)?;
            println!("已保存 {path} ({:.1}s)", samples.len() as f32 / rate as f32);
            Ok(())
        }
        Cmd::Chat { text } => {
            let rt = tokio::runtime::Runtime::new().context("创建 tokio runtime 失败")?;
            rt.block_on(async move {
                let mut brain = BrainKind::from_config(&cfg);
                for ev in brain.run(&text).await {
                    match ev {
                        BrainEvent::Delta(d) => print!("{d}"),
                        BrainEvent::Done(r) => println!("\n[回复] {r}"),
                        BrainEvent::Err(e) => eprintln!("\n[错误] {e}"),
                    }
                }
            });
            Ok(())
        }
    }
}

/// UI 主程序: 麦克风 + ASR + 大脑 + TTS 全部在后台线程跑,
/// macroquad 窗口在主线程渲染。
fn run_app(cfg: Config) -> Result<()> {
    let state: SharedState = Arc::new(Mutex::new(UiState {
        phase: Phase::Idle,
        status: "启动中...".into(),
        ..Default::default()
    }));

    // 检查模型是否齐全
    let models_ok = check_models(&cfg);
    if !models_ok {
        set_status(&state, "模型未下载,先运行: node scripts/download-models.js".into());
    }

    let (asr_tx, asr_rx) = flume::unbounded::<audio::input::AudioChunk>();
    let (ev_tx, ev_rx) = flume::unbounded::<asr::AsrEvent>();

    // 麦克风(失败不致命,界面仍可显示)
    if let Err(e) = audio::input::spawn_mic(asr_tx.clone(), state.clone()) {
        tracing::error!("麦克风不可用: {e:#}");
        set_status(&state, format!("麦克风不可用: {e:.80}"));
    }

    // ASR 工作线程
    if models_ok {
        if let Err(e) = asr::spawn_asr_worker(cfg.models.clone(), asr_rx, ev_tx) {
            tracing::error!("启动 ASR 失败: {e:#}");
        }
    }

    // 大脑 + TTS + 播放循环(tokio runtime 线程)
    let brain_state = state.clone();
    let brain_cfg = cfg.clone();
    thread::Builder::new()
        .name("brain-loop".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("创建 tokio runtime 失败");
            rt.block_on(brain_loop(brain_cfg, ev_rx, brain_state));
        })
        .context("启动大脑线程失败")?;

    ui::app::run_ui(cfg, state);
    Ok(())
}

fn check_models(cfg: &Config) -> bool {
    let dir = cfg.models.asr_dir.as_os_str().to_string_lossy();
    let tts_dir = cfg.models.tts_dir.as_os_str().to_string_lossy();
    [
        format!("{dir}/encoder-epoch-99-avg-1.int8.onnx"),
        format!("{dir}/decoder-epoch-99-avg-1.int8.onnx"),
        format!("{dir}/joiner-epoch-99-avg-1.int8.onnx"),
        cfg.models.vad_model.as_os_str().to_string_lossy().into_owned(),
        format!("{tts_dir}/{}", cfg.models.tts_model_file),
        format!("{tts_dir}/voices.bin"),
    ]
    .iter()
    .all(|p| Path::new(p).exists())
}

fn set_status(state: &SharedState, msg: String) {
    if let Ok(mut s) = state.lock() {
        s.status = msg;
    }
}

/// 核心闭环: ASR 事件 -> 大脑 -> TTS -> 播放 -> 状态机。
async fn brain_loop(
    cfg: Config,
    rx: flume::Receiver<asr::AsrEvent>,
    state: SharedState,
) {
    let mut brain = BrainKind::from_config(&cfg);

    let tts = match tts::Tts::new(&cfg.models) {
        Ok(t) => Some(Arc::new(t)),
        Err(e) => {
            tracing::error!("TTS 初始化失败: {e:#}");
            set_phase_error(&state, format!("TTS 初始化失败: {e:.100}"));
            None
        }
    };
    let player = match Player::new() {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::error!("播放器初始化失败: {e:#}");
            set_phase_error(&state, format!("播放器初始化失败: {e:.100}"));
            None
        }
    };

    loop {
        tokio::select! {
            ev = rx.recv_async() => {
                let Ok(ev) = ev else { break };
                match ev {
                    asr::AsrEvent::Partial(text) => {
                        if let Ok(mut s) = state.lock() {
                            if s.phase == Phase::Idle || s.phase == Phase::Listening {
                                s.phase = Phase::Listening;
                            }
                            s.asr_partial = text;
                        }
                    }
                    asr::AsrEvent::Final(text) => {
                        // 说话中被忽略(打断功能后续实现)
                        let busy = if let Ok(s) = state.lock() {
                            s.phase == Phase::Speaking || s.phase == Phase::Thinking
                        } else { true };
                        if busy {
                            tracing::info!("忽略 utterance(正在处理): {text}");
                            continue;
                        }
                        if let Ok(mut s) = state.lock() {
                            s.last_user_text = text.clone();
                            s.asr_partial.clear();
                            s.phase = Phase::Thinking;
                        }
                        tracing::info!("识别: {text}");

                        let events = brain.run(&text).await;
                        let reply = match events.iter().find_map(|e| match e {
                            BrainEvent::Done(r) => Some(r.clone()),
                            _ => None,
                        }) {
                            Some(r) => r,
                            None => {
                                let err = events.iter().find_map(|e| match e {
                                    BrainEvent::Err(m) => Some(m.clone()),
                                    _ => None,
                                }).unwrap_or_else(|| "无回复".into());
                                tracing::error!("大脑错误: {err}");
                                set_phase_error(&state, err);
                                continue;
                            }
                        };

                        // TTS 合成(阻塞调用放 spawn_blocking)
                        let (Some(tts), Some(player)) = (&tts, &player) else { continue };
                        let tts = tts.clone();
                        let reply_for_tts = reply.clone();
                        let tts_result = tokio::task::spawn_blocking(move || tts.synthesize(&reply_for_tts)).await;
                        let Ok(Some((samples, rate))) = tts_result else {
                            set_phase_error(&state, "语音合成失败".into());
                            continue;
                        };
                        if let Ok(mut s) = state.lock() {
                            s.reply_text = reply;
                            s.phase = Phase::Speaking;
                        }
                        player.play(samples, rate);
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {
                if let (Some(player), Ok(mut s)) = (&player, state.lock()) {
                    // 播放完回到待机
                    if s.phase == Phase::Speaking && player.is_idle() {
                        s.phase = Phase::Idle;
                        s.reply_text.clear();
                    }
                    // 待机且无声音时清掉残留的中间结果
                    if s.phase == Phase::Idle && s.mic_level < 0.05 && !s.asr_partial.is_empty() {
                        s.asr_partial.clear();
                    }
                }
            }
        }
    }
}

fn set_phase_error(state: &SharedState, msg: String) {
    if let Ok(mut s) = state.lock() {
        s.phase = Phase::Error;
        s.error = Some(msg);
    }
}
