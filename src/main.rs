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
        Cmd::AsrDiag { text } => asr::run_asr_diag(&cfg, &text),
        Cmd::Latency { text } => {
            let text = text.unwrap_or_else(|| "今天天气很好我们去公园散步吧".to_string());
            asr::run_latency_test(&cfg, &text)
        }
        Cmd::Speak { text } => {
            let text = text.unwrap_or_else(|| {
                "你好呀!我是你的像素伙伴小奶蛙。今天天气不错,我们去公园散步吧?好的,那就出发啦!".to_string()
            });
            let engine = tts::Tts::new(&cfg.models)?;
            let player = audio::output::Player::new()?;
            let sentences = split_sentences(&text);
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
        format!("{dir}/encoder.int8.onnx"),
        format!("{dir}/decoder.onnx"),
        format!("{dir}/joiner.int8.onnx"),
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
    // 大脑(共享: 流式请求在子任务里执行,避免阻塞事件循环;tokio 锁可在 await 间持有)
    let brain = Arc::new(tokio::sync::Mutex::new(BrainKind::from_config(&cfg)));

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
                        let t0 = std::time::Instant::now();
                        if let Ok(mut s) = state.lock() {
                            s.last_user_text = text.clone();
                            s.asr_partial.clear();
                            s.phase = Phase::Thinking;
                        }
                        tracing::info!("识别: {text}");

                        // ---- 大脑流式 → 按句 TTS 流式 ----
                        let (Some(tts), Some(player)) = (&tts, &player) else { continue };

                        // 大脑事件通道: Delta 实时到达,首字即触发第一句合成
                        let (ev_tx, ev_rx) = flume::unbounded::<BrainEvent>();
                        let brain2 = brain.clone();
                        let text2 = text.clone();
                        tokio::spawn(async move {
                            let mut b = brain2.lock().await;
                            b.run_streaming(&text2, ev_tx).await;
                        });

                        let (reply, played_any, _) =
                            run_stream_pipeline(tts, player, ev_rx, &state, t0).await;

                        if !played_any {
                            set_phase_error(&state, "语音合成失败".into());
                            continue;
                        }
                        tracing::info!(
                            "LATENCY 全部句子已入队 (总等待 {:.2}s, 共 {} 字)",
                            t0.elapsed().as_secs_f32(),
                            reply.chars().count()
                        );
                        if let Ok(mut s) = state.lock() {
                            s.reply_text = reply;
                            s.phase = Phase::Speaking;
                        }
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

/// 大脑事件流 → 按句 TTS → 播放队列 的流式管道。
/// 返回 (完整回复文本, 是否有音频入队, 首句音频入队耗时)。
async fn run_stream_pipeline(
    tts: &Arc<tts::Tts>,
    player: &audio::output::Player,
    ev_rx: flume::Receiver<BrainEvent>,
    state: &SharedState,
    t0: std::time::Instant,
) -> (String, bool, Option<std::time::Duration>) {
    // TTS 单消费者工作线程(保序): 收句子 → 合成 → 回传音频
    let (tts_req_tx, tts_req_rx) = flume::unbounded::<String>();
    let (tts_done_tx, tts_done_rx) = flume::unbounded::<Option<(Vec<f32>, u32)>>();
    let tts_worker = tts.clone();
    tokio::spawn(async move {
        while let Ok(s) = tts_req_rx.recv_async().await {
            let tts = tts_worker.clone();
            let out = tokio::task::spawn_blocking(move || tts.synthesize(&s))
                .await
                .ok()
                .flatten();
            let _ = tts_done_tx.send(out);
        }
    });

    let mut reply = String::new();
    let mut sentence_buf = String::new();
    let mut pending = 0usize;
    let mut played_any = false;
    let mut brain_done = false;
    let mut aborted = false;
    let mut first_audio: Option<std::time::Duration> = None;
    // 大脑通道是否已关闭(发完 Done 后通道断开是正常现象,不能提前退出,
    // 必须等 TTS 在途任务全部回来)
    let mut brain_closed = false;

    while !aborted {
        tokio::select! {
            ev = ev_rx.recv_async(), if !brain_closed => match ev {
                Ok(BrainEvent::Delta(d)) => {
                    reply.push_str(&d);
                    sentence_buf.push_str(&d);
                    // 气泡实时更新
                    if let Ok(mut s) = state.lock() {
                        s.reply_text = reply.clone();
                    }
                    while let Some(s) = take_sentence(&mut sentence_buf) {
                        pending += 1;
                        let _ = tts_req_tx.send(s);
                    }
                }
                Ok(BrainEvent::Done(_)) => {
                    if !sentence_buf.trim().is_empty() {
                        pending += 1;
                        let _ = tts_req_tx.send(std::mem::take(&mut sentence_buf));
                    }
                    brain_done = true;
                    brain_closed = true;
                    if pending == 0 {
                        break;
                    }
                }
                Ok(BrainEvent::Err(e)) => {
                    tracing::error!("大脑错误: {e}");
                    brain_closed = true;
                    if played_any {
                        brain_done = true;
                        if pending == 0 {
                            break;
                        }
                    } else {
                        set_phase_error(state, e);
                        aborted = true;
                    }
                }
                Err(_) => {
                    // 通道断开(大脑任务结束): 若有 TTS 在途则继续等
                    brain_closed = true;
                    if pending == 0 {
                        break;
                    }
                }
            },
            audio = tts_done_rx.recv_async() => {
                if let Ok(Some((mut samples, rate))) = audio {
                    if played_any {
                        // 句间停顿 0.25s
                        let pause = vec![0f32; rate as usize / 4];
                        samples.splice(0..0, pause);
                    }
                    if !played_any {
                        tracing::info!(
                            "LATENCY 识别完成->首句开播 {:.2}s",
                            t0.elapsed().as_secs_f32()
                        );
                        if let Ok(mut s) = state.lock() {
                            s.phase = Phase::Speaking;
                        }
                    }
                    tracing::info!(
                        "LATENCY 收到合成音频 {:.1}s (剩余 {pending})",
                        samples.len() as f32 / rate as f32
                    );
                    if first_audio.is_none() {
                        first_audio = Some(t0.elapsed());
                    }
                    player.queue(samples, rate);
                    played_any = true;
                } else {
                    tracing::warn!("LATENCY 收到空/失败音频结果 (pending {pending})");
                }
                pending = pending.saturating_sub(1);
                if brain_done && pending == 0 {
                    break;
                }
            }
        }
    }
    tracing::info!("LATENCY 管道结束 played={played_any} pending={pending} aborted={aborted}");
    (reply, played_any, first_audio)
}

/// 按句末标点切分回复;无标点时按逗号软切(≥6字),超过 25 字无任何停顿则硬切。
/// 保证 TTS 流式粒度(避免一整段攒到结尾才合成)。
fn split_sentences(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = 0usize;
    for c in text.chars() {
        cur.push(c);
        chars += 1;
        let hard_break = matches!(c, '。' | '！' | '？' | '…' | '!' | '?' | '.') && chars >= 2;
        let soft_break = matches!(c, '，' | ',' | '、' | '；' | ';') && chars >= 6;
        if hard_break || soft_break || chars >= 25 {
            out.push(std::mem::take(&mut cur));
            chars = 0;
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// 流式增量版: 从累积缓冲中取出一句完整的话(与 split_sentences 同规则)。
fn take_sentence(buf: &mut String) -> Option<String> {
    let mut chars = 0usize;
    for (i, c) in buf.char_indices() {
        chars += 1;
        let hard_break = matches!(c, '。' | '！' | '？' | '…' | '!' | '?' | '.') && chars >= 2;
        let soft_break = matches!(c, '，' | ',' | '、' | '；' | ';') && chars >= 6;
        if hard_break || soft_break || chars >= 25 {
            let end = i + c.len_utf8();
            let s = buf[..end].to_string();
            buf.drain(..end);
            return Some(s);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::split_sentences;

    #[test]
    fn split_sentences_breaks_and_merges() {
        let s = "今天天气很好我们去公园散步吧。明天可能会下雨,记得带伞!出门前看一下天气预报?好的";
        let parts = split_sentences(s);
        assert!(parts.len() >= 4, "应至少切出 4 句: {parts:?}");
        assert!(parts.iter().all(|p| !p.trim().is_empty()));
        assert_eq!(parts.concat(), s, "切分不应丢字");
    }

    #[test]
    fn split_sentences_hard_cuts_long() {
        let long = "啊".repeat(50);
        let parts = split_sentences(&long);
        assert!(parts.iter().all(|p| p.chars().count() <= 25));
        assert_eq!(parts.iter().map(|p| p.chars().count()).sum::<usize>(), 50);
    }

    #[test]
    fn take_sentence_incremental() {
        use super::take_sentence;
        let mut buf = String::new();
        // 逐字喂入,模拟 LLM 流式
        for c in "今天天气不错。明天会下雨,记得带伞!".chars() {
            buf.push(c);
            take_sentence(&mut buf); // 有完整句时取出
        }
        // 流结束: 冲掉剩余
        let rest = take_sentence(&mut buf);
        assert!(rest.is_none() || !rest.unwrap().trim().is_empty());
        assert!(buf.is_empty(), "缓冲应被取空: {buf}");
    }

    /// 复现测试: 模拟大脑流式推 Delta,验证"合成结果能否回到播放循环"。
    /// 用户反馈: TTS 合成正常但播放不出,程序报"语音合成失败"。
    #[tokio::test]
    async fn stream_pipeline_plays_audio() {
        use crate::audio::output::Player;
        use crate::brain::BrainEvent;
        use crate::config::Config;
        use crate::state::{SharedState, UiState};
        use crate::tts;
        use std::sync::{Arc, Mutex};

        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .try_init();

        let cfg = Config::load().expect("config 加载失败");
        let tts = Arc::new(tts::Tts::new(&cfg.models).expect("TTS 初始化失败"));
        let player = Arc::new(Player::new().expect("播放器初始化失败"));
        let state: SharedState = Arc::new(Mutex::new(UiState::default()));

        let (ev_tx, ev_rx) = flume::unbounded::<BrainEvent>();
        // 模拟 LLM 流式: 逐字推送 Delta,最后 Done
        let text = "你好呀!我是你的像素伙伴小奶蛙。今天天气不错呢?";
        let text2 = text.to_string();
        tokio::spawn(async move {
            for c in text2.chars() {
                let _ = ev_tx.send(BrainEvent::Delta(c.to_string()));
                tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            }
            let _ = ev_tx.send(BrainEvent::Done(text2.clone()));
        });

        let (reply, played, _) = super::run_stream_pipeline(
            &tts,
            &player,
            ev_rx,
            &state,
            std::time::Instant::now(),
        )
        .await;
        {
            let st = state.lock().unwrap();
            tracing::warn!("DIAG reply={reply:?} played={played} reply_text={:?}", st.reply_text);
        }
        assert!(played, "应有音频入队播放!");
        assert_eq!(reply.chars().count(), text.chars().count(), "回复文本应完整");

        // 等播放结束(验证真的在播)
        let mut waited = 0.0f32;
        while !player.is_idle() && waited < 30.0 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            waited += 0.1;
        }
        assert!(player.is_idle(), "播放应在 30s 内结束");
    }

    /// 无句号回复也必须在流式过程中出声,不能等全部转换完成。
    /// 慢速流式(150ms/字)模拟真实 LLM 节奏,只有逗号、没有句末标点。
    #[tokio::test]
    async fn stream_pipeline_incremental_no_punct() {
        use crate::audio::output::Player;
        use crate::brain::BrainEvent;
        use crate::config::Config;
        use crate::state::{SharedState, UiState};
        use crate::tts;
        use std::sync::{Arc, Mutex};

        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .try_init();
        let cfg = Config::load().expect("config 加载失败");
        let tts = Arc::new(tts::Tts::new(&cfg.models).expect("TTS 初始化失败"));
        let player = Arc::new(Player::new().expect("播放器初始化失败"));
        let state: SharedState = Arc::new(Mutex::new(UiState::default()));

        // 只有逗号的长回复: 逗号软切(≥6字)保证第一段尽早合成
        let text = "今天天气不错,明天会下雨记得带伞,出门前看一下天气预报,晚上早点回家";
        let (ev_tx, ev_rx) = flume::unbounded::<BrainEvent>();
        let text2 = text.to_string();
        tokio::spawn(async move {
            for c in text2.chars() {
                let _ = ev_tx.send(BrainEvent::Delta(c.to_string()));
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
            let _ = ev_tx.send(BrainEvent::Done(text2.clone()));
        });

        let t0 = std::time::Instant::now();
        let (reply, played, first_audio) =
            super::run_stream_pipeline(&tts, &player, ev_rx, &state, t0).await;
        let total = t0.elapsed();
        assert!(played, "应有音频入队播放!");
        assert_eq!(reply.chars().count(), text.chars().count(), "回复文本应完整");
        let fa = first_audio.expect("应记录首句入队时间");
        // 关键断言: 首句入队必须远早于管道结束(没有攒到结尾才出声)
        assert!(
            fa < total - std::time::Duration::from_millis(1500),
            "首句入队 {fa:?} 应比管道结束 {total:?} 早至少 1.5s(疑似等全部转换)"
        );

        let mut waited = 0.0f32;
        while !player.is_idle() && waited < 30.0 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            waited += 0.1;
        }
        assert!(player.is_idle(), "播放应在 30s 内结束");
    }
}

fn set_phase_error(state: &SharedState, msg: String) {
    if let Ok(mut s) = state.lock() {
        s.phase = Phase::Error;
        s.error = Some(msg);
    }
}
