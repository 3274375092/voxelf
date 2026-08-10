//! 语音对话主管线: ASR 事件 → 状态机 → 大脑流式 → 分句 → TTS 合成 → 播放队列。
//!
//! 设计:
//! - `next_phase` 是纯函数状态机,事件与状态一一映射,可独立测试。
//! - `brain_loop` 把 ASR 事件接到大脑,并驱动 `run_stream_pipeline`。
//! - `run_stream_pipeline` 通过两个窄接口(synthesize / enqueue)与真实 I/O 解耦:
//!   生产环境传入 `tts::Tts` + `audio::output::Player`,测试传入假合成/假播放,
//!   无需模型文件与音频设备即可验证流式语义(切句、首句早出声、不丢字)。

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio::output::Player;
use crate::brain::{BrainEvent, BrainKind};
use crate::config::Config;
use crate::state::{Phase, SharedState};

/// 事件 → 下一状态(纯函数,状态机可测试):
/// - SpeechStarted/Partial: 仅在 Idle/Listening 时进入 Listening(开口即有反馈)
/// - Final: Speaking/Thinking 时视为 busy 忽略(打断后续实现),否则进入 Thinking
pub fn next_phase(ev: &crate::asr::AsrEvent, cur: Phase) -> Option<Phase> {
    match ev {
        crate::asr::AsrEvent::SpeechStarted | crate::asr::AsrEvent::Partial(_) => {
            if cur == Phase::Idle || cur == Phase::Listening {
                Some(Phase::Listening)
            } else {
                None
            }
        }
        crate::asr::AsrEvent::Final(_) => {
            if cur == Phase::Speaking || cur == Phase::Thinking {
                None
            } else {
                Some(Phase::Thinking)
            }
        }
    }
}

/// 核心闭环: ASR 事件 -> 大脑 -> TTS -> 播放 -> 状态机。
pub async fn brain_loop(
    cfg: Config,
    rx: flume::Receiver<crate::asr::AsrEvent>,
    state: SharedState,
) {
    // 大脑(共享: 流式请求在子任务里执行,避免阻塞事件循环;tokio 锁可在 await 间持有)
    let brain = Arc::new(tokio::sync::Mutex::new(BrainKind::from_config(&cfg)));

    let tts = match crate::tts::Tts::new(&cfg.models) {
        Ok(t) => Some(Arc::new(t)),
        Err(e) => {
            tracing::error!("TTS 初始化失败: {e:#}");
            set_phase_error(&state, format!("TTS 初始化失败: {e:.100}"));
            None
        }
    };
    let player = match Player::new() {
        Ok(p) => Some(Arc::new(p)),
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
                    crate::asr::AsrEvent::SpeechStarted => {
                        if let Ok(mut s) = state.lock() {
                            if let Some(p) = next_phase(&ev, s.phase) {
                                s.phase = p;
                            }
                        }
                    }
                    crate::asr::AsrEvent::Partial(ref text) => {
                        if let Ok(mut s) = state.lock() {
                            if let Some(p) = next_phase(&ev, s.phase) {
                                s.phase = p;
                            }
                            s.asr_partial = text.clone();
                        }
                    }
                    crate::asr::AsrEvent::Final(ref text) => {
                        // 说话中被忽略(打断功能后续实现)
                        let busy = if let Ok(s) = state.lock() {
                            s.phase == Phase::Speaking || s.phase == Phase::Thinking
                        } else { true };
                        if busy {
                            tracing::info!("忽略 utterance(正在处理): {text}");
                            continue;
                        }
                        let t0 = Instant::now();
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

                        // 生产 I/O: 真实 TTS 合成 + 真实播放队列
                        let tts_io = tts.clone();
                        let player_io = player.clone();
                        let (reply, played_any, _) = run_stream_pipeline(
                            move |s: &str| tts_io.synthesize(s),
                            move |samples, rate| player_io.queue(samples, rate),
                            ev_rx,
                            &state,
                            t0,
                        )
                        .await;

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
            _ = tokio::time::sleep(Duration::from_millis(200)) => {
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
///
/// I/O 通过两个闭包注入,便于无模型/无声卡的纯逻辑测试:
/// - `synthesize`: 把一句话合成为 (PCM f32, 采样率),阻塞调用(内部 spawn_blocking)。
/// - `enqueue`:   把一段音频追加到播放队列(不打断当前播放)。
///
/// 返回 (完整回复文本, 是否有音频入队, 首句音频入队耗时)。
pub async fn run_stream_pipeline<S, E>(
    synthesize: S,
    enqueue: E,
    ev_rx: flume::Receiver<BrainEvent>,
    state: &SharedState,
    t0: Instant,
) -> (String, bool, Option<Duration>)
where
    S: Fn(&str) -> Option<(Vec<f32>, u32)> + Send + Sync + 'static,
    E: Fn(Vec<f32>, u32),
{
    // TTS 单消费者工作线程(保序): 收句子 → 合成 → 回传音频
    let synthesize = Arc::new(synthesize);
    let (tts_req_tx, tts_req_rx) = flume::unbounded::<String>();
    let (tts_done_tx, tts_done_rx) = flume::unbounded::<Option<(Vec<f32>, u32)>>();
    tokio::spawn({
        let synthesize = synthesize.clone();
        async move {
            while let Ok(s) = tts_req_rx.recv_async().await {
                let f = synthesize.clone();
                let out = tokio::task::spawn_blocking(move || f(&s))
                    .await
                    .ok()
                    .flatten();
                let _ = tts_done_tx.send(out);
            }
        }
    });

    let mut reply = String::new();
    let mut sentence_buf = String::new();
    let mut pending = 0usize;
    let mut played_any = false;
    let mut brain_done = false;
    let mut aborted = false;
    let mut first_audio: Option<Duration> = None;
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
                Ok(BrainEvent::Working(tool)) => {
                    // agent 正在执行工具: 切"干活"动画,文本不进入 TTS 流
                    tracing::info!("agent 工具: {tool}");
                    if let Ok(mut s) = state.lock() {
                        s.phase = Phase::Working;
                        s.status = format!("正在{tool}...");
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
                    enqueue(samples, rate);
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

/// 第一处句子断点(含断点标点)的字节索引;无断点返回 None。
/// 规则: 句末标点(≥2字)硬断;逗号类(≥6字)软断;超过 25 字无任何停顿硬断。
/// 保证 TTS 流式粒度(避免一整段攒到结尾才合成)。
fn sentence_break(buf: &str) -> Option<usize> {
    let mut chars = 0usize;
    for (i, c) in buf.char_indices() {
        chars += 1;
        let hard_break = matches!(c, '。' | '！' | '？' | '…' | '!' | '?' | '.') && chars >= 2;
        let soft_break = matches!(c, '，' | ',' | '、' | '；' | ';') && chars >= 6;
        if hard_break || soft_break || chars >= 25 {
            return Some(i + c.len_utf8());
        }
    }
    None
}

/// 按句末标点切分整段回复(批量版)。
pub fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(end) = sentence_break(rest) {
        out.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    if !rest.trim().is_empty() {
        out.push(rest.to_string());
    }
    out
}

/// 流式增量版: 从累积缓冲中取出一句完整的话(与 split_sentences 同规则)。
fn take_sentence(buf: &mut String) -> Option<String> {
    let end = sentence_break(buf)?;
    let s = buf[..end].to_string();
    buf.drain(..end);
    Some(s)
}

fn set_phase_error(state: &SharedState, msg: String) {
    if let Ok(mut s) = state.lock() {
        s.phase = Phase::Error;
        s.error = Some(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::{next_phase, run_stream_pipeline, split_sentences, take_sentence};
    use crate::asr::AsrEvent;
    use crate::audio::output::Player;
    use crate::brain::BrainEvent;
    use crate::config::Config;
    use crate::state::{Phase, SharedState, UiState};
    use crate::tts;
    use std::sync::{Arc, Mutex};

    /// 状态机测试: 事件 → 状态转换必须与动画匹配。
    #[test]
    fn state_machine_listening_on_speech_start() {
        // 开口瞬间(VAD) → 立即聆听动画
        assert_eq!(
            next_phase(&AsrEvent::SpeechStarted, Phase::Idle),
            Some(Phase::Listening)
        );
        assert_eq!(
            next_phase(&AsrEvent::SpeechStarted, Phase::Listening),
            Some(Phase::Listening)
        );
        assert_eq!(
            next_phase(&AsrEvent::Partial("你".into()), Phase::Idle),
            Some(Phase::Listening)
        );
        // 处理中(思考/说话)不被打断回聆听
        assert_eq!(next_phase(&AsrEvent::SpeechStarted, Phase::Thinking), None);
        assert_eq!(next_phase(&AsrEvent::SpeechStarted, Phase::Speaking), None);
        assert_eq!(next_phase(&AsrEvent::SpeechStarted, Phase::Working), None);
    }

    #[test]
    fn state_machine_final_to_thinking() {
        assert_eq!(
            next_phase(&AsrEvent::Final("你好".into()), Phase::Listening),
            Some(Phase::Thinking)
        );
        assert_eq!(
            next_phase(&AsrEvent::Final("你好".into()), Phase::Idle),
            Some(Phase::Thinking)
        );
        // busy: 上一句还在处理,新 Final 被忽略
        assert_eq!(next_phase(&AsrEvent::Final("你好".into()), Phase::Speaking), None);
        assert_eq!(next_phase(&AsrEvent::Final("你好".into()), Phase::Thinking), None);
    }

    /// 完整对话循环: Idle → 开口 → Listening → Final → Thinking → 播放 Speaking → Idle
    #[test]
    fn state_machine_full_conversation_cycle() {
        let mut phase = Phase::Idle;
        phase = next_phase(&AsrEvent::SpeechStarted, phase).unwrap();
        assert_eq!(phase, Phase::Listening, "开口应立刻聆听");
        phase = next_phase(&AsrEvent::Partial("你好".into()), phase).unwrap();
        assert_eq!(phase, Phase::Listening);
        phase = next_phase(&AsrEvent::Final("你好".into()), phase).unwrap();
        assert_eq!(phase, Phase::Thinking, "识别完成应思考");
        // 播放结束回到 Idle(由播放器空闲轮询驱动,不在事件机内)
        phase = Phase::Idle;
        assert_eq!(phase, Phase::Idle);
    }

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

    /// 切句与流式增量切句必须共享同一断点规则(防止两套逻辑漂移)。
    #[test]
    fn split_and_take_agree_on_boundaries() {
        for s in [
            "你好。世界!",
            "今天天气不错,明天会下雨",
            "a".repeat(30).as_str(),
            "好的",
            "嗯",
        ] {
            let batch = split_sentences(s);
            let mut buf = s.to_string();
            let mut incr = Vec::new();
            while let Some(sent) = take_sentence(&mut buf) {
                incr.push(sent);
            }
            if !buf.trim().is_empty() {
                incr.push(buf);
            }
            assert_eq!(batch, incr, "批量与增量切句结果应一致: {s:?}");
        }
    }

    /// 复现测试(纯逻辑,无模型/无声卡): 模拟大脑流式推 Delta,
    /// 验证"合成结果能否回到播放循环"。
    #[tokio::test]
    async fn stream_pipeline_plays_audio_hermetic() {
        let state: SharedState = Arc::new(Mutex::new(UiState::default()));

        let (ev_tx, ev_rx) = flume::unbounded::<BrainEvent>();
        // 模拟 LLM 流式: 逐字推送 Delta,最后 Done
        let text = "你好呀!我是你的像素伙伴小可爱。今天天气不错呢?";
        let text2 = text.to_string();
        tokio::spawn(async move {
            for c in text2.chars() {
                let _ = ev_tx.send(BrainEvent::Delta(c.to_string()));
                tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            }
            let _ = ev_tx.send(BrainEvent::Done(text2.clone()));
        });

        // 假 TTS: 立即返回 1s 静音;假播放: 记录每次入队的采样数
        let queued: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
        let q2 = queued.clone();
        let (reply, played, _) = run_stream_pipeline(
            |s: &str| {
                assert!(!s.trim().is_empty(), "不应合成空句");
                Some((vec![0f32; 16000], 16000u32))
            },
            move |samples, rate| {
                q2.lock().unwrap().push(samples.len() as u32);
                assert_eq!(rate, 16000);
            },
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
        assert!(!queued.lock().unwrap().is_empty(), "假播放器应收到音频");
        // 多句文本应按句切分多次合成,而不是整段一次
        assert!(
            queued.lock().unwrap().len() >= 3,
            "应至少 3 段音频(按句切分): {:?}",
            queued.lock().unwrap()
        );
    }

    /// 无句号回复也必须在流式过程中出声,不能等全部转换完成。
    /// 慢速流式(150ms/字)模拟真实 LLM 节奏,只有逗号、没有句末标点。
    #[tokio::test]
    async fn stream_pipeline_incremental_no_punct() {
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

        let queued: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
        let q2 = queued.clone();
        let t0 = std::time::Instant::now();
        let (reply, played, first_audio) = run_stream_pipeline(
            |_s: &str| Some((vec![0f32; 16000], 16000u32)),
            move |samples, _rate| {
                q2.lock().unwrap().push(samples.len() as u32);
            },
            ev_rx,
            &state,
            t0,
        )
        .await;
        let total = t0.elapsed();
        assert!(played, "应有音频入队播放!");
        assert_eq!(reply.chars().count(), text.chars().count(), "回复文本应完整");
        let fa = first_audio.expect("应记录首句入队时间");
        // 关键断言: 首句入队必须远早于管道结束(没有攒到结尾才出声)
        assert!(
            fa < total - std::time::Duration::from_millis(1500),
            "首句入队 {fa:?} 应比管道结束 {total:?} 早至少 1.5s(疑似等全部转换)"
        );
        assert!(!queued.lock().unwrap().is_empty(), "应有音频入队");
    }

    /// 大脑中途报错且尚未出声: 应进入 Error 状态并中止管道。
    #[tokio::test]
    async fn stream_pipeline_error_before_audio_sets_error_phase() {
        let state: SharedState = Arc::new(Mutex::new(UiState::default()));
        let (ev_tx, ev_rx) = flume::unbounded::<BrainEvent>();
        tokio::spawn(async move {
            let _ = ev_tx.send(BrainEvent::Delta("你好".into()));
            let _ = ev_tx.send(BrainEvent::Err("API 超时".into()));
        });
        let (_, played, _) = run_stream_pipeline(
            |_s: &str| Some((vec![0f32; 16000], 16000u32)),
            |_samples, _rate| {},
            ev_rx,
            &state,
            std::time::Instant::now(),
        )
        .await;
        assert!(!played, "出错时不应入队音频");
        let st = state.lock().unwrap();
        assert_eq!(st.phase, Phase::Error);
        assert_eq!(st.error.as_deref(), Some("API 超时"));
    }

    /// 端到端真实链路(需模型 + 音频设备): 缺任一即跳过。
    /// 用户反馈: TTS 合成正常但播放不出,程序报"语音合成失败"。
    #[tokio::test]
    async fn stream_pipeline_end_to_end_real() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .try_init();

        let cfg = match Config::load() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("SKIP: config 加载失败: {e}");
                return;
            }
        };
        let tts = match tts::Tts::new(&cfg.models) {
            Ok(t) => Arc::new(t),
            Err(e) => {
                eprintln!("SKIP: TTS 不可用(模型缺失?): {e}");
                return;
            }
        };
        let player = match Player::new() {
            Ok(p) => Arc::new(p),
            Err(e) => {
                eprintln!("SKIP: 音频设备不可用: {e}");
                return;
            }
        };
        let state: SharedState = Arc::new(Mutex::new(UiState::default()));

        let (ev_tx, ev_rx) = flume::unbounded::<BrainEvent>();
        // 模拟 LLM 流式: 逐字推送 Delta,最后 Done
        let text = "你好呀!我是你的像素伙伴小可爱。今天天气不错呢?";
        let text2 = text.to_string();
        tokio::spawn(async move {
            for c in text2.chars() {
                let _ = ev_tx.send(BrainEvent::Delta(c.to_string()));
                tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            }
            let _ = ev_tx.send(BrainEvent::Done(text2.clone()));
        });

        let tts_io = tts.clone();
        let player_io = player.clone();
        let (reply, played, _) = super::run_stream_pipeline(
            move |s: &str| tts_io.synthesize(s),
            move |samples, rate| player_io.queue(samples, rate),
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

    /// 断点规则边界: 25 字硬切、2 字以下不切、逗号软切阈值。
    #[test]
    fn sentence_break_boundaries() {
        // 逗号软切需 ≥6 字(含逗号)
        assert_break("你好你好,", None); // 5 字,不够
        assert_break("你你你你你,", Some(16)); // 6 字 → 软切
        // 句末标点 ≥2 字即可
        assert_break("你好。", Some(9));
        // 25 字硬切
        assert_break(&"啊".repeat(25), Some(25 * 3));
    }

    fn assert_break(s: &str, want: Option<usize>) {
        assert_eq!(super::sentence_break(s), want, "断点应一致: {s:?}");
    }
}
