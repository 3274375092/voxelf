mod audio;
mod asr;
mod brain;
mod config;
mod diag;
mod pipeline;
mod state;
mod tray;
mod tts;
mod ui;

use config::Config;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

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
    /// 图形界面主程序(默认,桌宠模式)
    Run,
    /// 对 wav 文件跑语音识别
    AsrFile { wav: String },
    /// 文本合成语音,保存为 wav
    Tts { text: String, out: Option<String> },
    /// 纯文本对话(不走语音)
    Chat { text: String },
    /// ASR 定位测试: TTS 合成已知文本,三模式对照找"吞句尾"问题
    AsrDiag { text: String },
    /// 延迟定位测试: 测 TTS/ASR/各阶段耗时
    Latency { text: Option<String> },
    /// 流式朗读测试: 分句合成并播放(验证 TTS 流式)
    Speak { text: Option<String> },
    /// TTS 基准: 预热 + 稳态合成耗时(对比 kokoro/vits)
    TtsBench,
}

fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();
    let cfg = config::Config::load()?;

    match cli.cmd.unwrap_or(Cmd::Run) {
        Cmd::Run => run_app(cfg),
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
                        BrainEvent::Working(t) => println!("\n[工具] {t}"),
                    }
                }
            });
            Ok(())
        }
        Cmd::AsrDiag { text } => diag::run_asr_diag(&cfg, &text),
        Cmd::Latency { text } => {
            let text = text.unwrap_or_else(|| "今天天气很好我们去公园散步吧".to_string());
            diag::run_latency_test(&cfg, &text)
        }
        Cmd::Speak { text } => {
            let text = text.unwrap_or_else(|| {
                "你好呀!我是你的像素伙伴小可爱。今天天气不错,我们去公园散步吧?好的,那就出发啦!".to_string()
            });
            let engine = tts::Tts::new(&cfg.models)?;
            let player = audio::output::Player::new()?;
            let sentences = pipeline::split_sentences(&text);
            let t0 = std::time::Instant::now();
            for (i, s) in sentences.iter().enumerate() {
                let (mut samples, rate) = engine.synthesize(s).context("语音合成失败")?;
                if i > 0 {
                    let pause = vec![0f32; rate as usize / 4];
                    samples.splice(0..0, pause);
                }
                if i == 0 {
                    tracing::info!("LATENCY 首句合成完成 {:.2}s,开始播放", t0.elapsed().as_secs_f32());
                }
                player.queue(samples, rate);
            }
            tracing::info!("LATENCY 全部 {} 句已入队, 播放中...", sentences.len());
            while !player.is_idle() {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            tracing::info!("播放完成");
            Ok(())
        }
        Cmd::TtsBench => {
            let engine = tts::Tts::new(&cfg.models)?;
            // 预热(首个调用含模型初始化开销)
            let t0 = std::time::Instant::now();
            let _ = engine.synthesize("嗯");
            println!("预热(首个调用): {:.2}s", t0.elapsed().as_secs_f32());
            for text in [
                "今天天气很好我们去公园散步吧",
                "你能帮我查一下明天的天气吗",
                "好的那我们明天见",
            ] {
                let t0 = std::time::Instant::now();
                if let Some((samples, rate)) = engine.synthesize(text) {
                    println!(
                        "稳态 {:.2}s ({}字 -> 音频 {:.1}s)",
                        t0.elapsed().as_secs_f32(),
                        text.chars().count(),
                        samples.len() as f32 / rate as f32
                    );
                }
            }
            Ok(())
        }
    }
}

/// 日志同时输出到 stderr 与 voxelf.log(桌面启动时定位问题用)。
fn init_logging() {
    // 日志文件只保留最近 ~10MB:超限时把旧的转成 voxelf.log.1(不追加新内容)
    let log_file = rotate_log_file();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(move || -> Box<dyn std::io::Write> {
            if let Some(f) = &log_file {
                struct Dual(std::io::Stderr, std::fs::File);
                impl std::io::Write for Dual {
                    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                        let _ = self.0.write(buf);
                        self.1.write(buf)
                    }
                    fn flush(&mut self) -> std::io::Result<()> {
                        let _ = self.0.flush();
                        self.1.flush()
                    }
                }
                Box::new(Dual(std::io::stderr(), f.try_clone().unwrap()))
            } else {
                Box::new(std::io::stderr())
            }
        })
        .init();
}

const LOG_FILE: &str = "voxelf.log";
const LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// 打开日志文件;若现有日志超过上限,把旧文件改名为 voxelf.log.1 再新建。
/// 失败时返回 None(仅 stderr 输出,不阻塞启动)。
fn rotate_log_file() -> Option<std::fs::File> {
    if let Ok(meta) = std::fs::metadata(LOG_FILE)
        && meta.len() > LOG_MAX_BYTES
    {
        let _ = std::fs::rename(LOG_FILE, format!("{LOG_FILE}.1"));
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_FILE)
        .ok()
}

/// UI 主程序: 麦克风 + ASR + 大脑 + TTS 全部在后台线程跑,
/// eframe 桌宠窗口在主线程渲染。
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
    if models_ok
        && let Err(e) = asr::spawn_asr_worker(cfg.models.clone(), asr_rx, ev_tx)
    {
        tracing::error!("启动 ASR 失败: {e:#}");
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
            rt.block_on(pipeline::brain_loop(brain_cfg, ev_rx, brain_state));
        })
        .context("启动大脑线程失败")?;

    ui::app::run_ui(state)
    .map_err(|e| anyhow::anyhow!("eframe 启动失败: {e}"))?;
    Ok(())
}

fn check_models(cfg: &Config) -> bool {
    let dir = cfg.models.asr_dir.as_os_str().to_string_lossy();
    let tts_dir = cfg.models.tts_dir.as_os_str().to_string_lossy();
    let mut checks = vec![
        format!("{dir}/encoder.int8.onnx"),
        format!("{dir}/decoder.onnx"),
        format!("{dir}/joiner.int8.onnx"),
        cfg.models.vad_model.as_os_str().to_string_lossy().into_owned(),
    ];
    // TTS 按引擎检查不同的文件
    if cfg.models.tts_kind == "vits" {
        checks.push(format!("{tts_dir}/model.onnx"));
        checks.push(format!("{tts_dir}/tokens.txt"));
        checks.push(format!("{tts_dir}/lexicon.txt"));
    } else {
        checks.push(format!("{tts_dir}/{}", cfg.models.tts_model_file));
        checks.push(format!("{tts_dir}/voices.bin"));
    }
    checks.iter().all(|p| Path::new(p).exists())
}

fn set_status(state: &SharedState, msg: String) {
    if let Ok(mut s) = state.lock() {
        s.status = msg;
    }
}
