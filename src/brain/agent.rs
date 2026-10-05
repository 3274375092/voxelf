use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout};

use super::adapters::{self, CliDialect, LineKind};
use super::BrainEvent;
use crate::config::AgentCfg;

/// Agent 大脑:驱动一个命令行 coding agent 子进程干活。
///
/// 默认协议(jcode)是常驻 `jcode repl` 子进程:每轮请求写一行 stdin、按行读
/// stdout,进程复用避开冷启动。协议差异(探测规则、启动参数、输出方言、
/// 是否常驻)全部封装在 `adapters::CliDialect`,本结构只负责进程生命周期:
/// 启动/重启/超时/行循环。one-shot 协议(DSH 等一次性 CLI)每轮新起进程,
/// 收集 stdout 全文作为最终回复。
pub struct AgentBrain {
    cfg: AgentCfg,
    dialect: Box<dyn CliDialect>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    reader: Option<Lines<BufReader<ChildStdout>>>,
    /// 是否已完成启动排空(ready 状态,仅常驻 repl 协议)
    started: bool,
}

impl AgentBrain {
    pub fn new(cfg: &AgentCfg) -> Self {
        Self {
            cfg: cfg.clone(),
            dialect: adapters::from_config(cfg),
            child: None,
            stdin: None,
            reader: None,
            started: false,
        }
    }

    /// 探测 agent CLI 是否可用(用户机器上装了对应命令才启用 agent 模式)。
    /// 探测规则由协议适配器决定(jcode 会回退同目录便携版 jcode.exe)。
    pub fn available(cfg: &AgentCfg) -> bool {
        adapters::from_config(cfg).available(cfg)
    }

    /// 流式运行一轮。常驻 repl 协议复用进程、逐行解析;one-shot 协议
    /// 每轮新起进程,stdout 全文作为一次 Delta + Done。
    pub async fn run_streaming(&mut self, text: &str, tx: flume::Sender<BrainEvent>) {
        if self.dialect.persistent() {
            self.run_repl(text, tx).await;
        } else {
            self.run_one_shot(text, tx).await;
        }
    }

    /// 启动(或重启)repl 进程。不预读输出: banner 与启动 prompt 由
    /// run_repl 的分类逻辑惰性丢弃(它们不会触发文本事件)。
    async fn ensure_started(&mut self) -> Result<()> {
        if self.started {
            return Ok(());
        }
        self.kill().await;

        let mut cmd = tokio::process::Command::new(self.dialect.resolve_command(&self.cfg));
        cmd.args(self.dialect.spawn_args(&self.cfg))
            .current_dir(&self.cfg.workdir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut child = cmd
            .spawn()
            .with_context(|| format!("启动 agent {} 失败", self.cfg.command))?;
        let stdin = child.stdin.take().context("agent stdin 不可用")?;
        let stdout = child.stdout.take().context("agent stdout 不可用")?;
        self.child = Some(child);
        self.stdin = Some(stdin);
        self.reader = Some(BufReader::new(stdout).lines());
        self.started = true;
        Ok(())
    }

    /// 杀掉子进程并清空句柄(下次调用会重新启动)。
    async fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill().await;
            let _ = c.wait().await;
        }
        self.stdin = None;
        self.reader = None;
        self.started = false;
    }

    /// 常驻 repl 协议:写一行 stdin,逐行分类 stdout 直到本轮结束。
    async fn run_repl(&mut self, text: &str, tx: flume::Sender<BrainEvent>) {
        let send = |e: BrainEvent| {
            let _ = tx.send(e);
        };

        if let Err(e) = self.ensure_started().await {
            send(BrainEvent::Err(format!("{e:#}")));
            return;
        }

        // 写输入
        if let Err(e) = self.stdin.as_mut().expect("stdin 存在").write_all(text.as_bytes()).await {
            self.kill().await;
            send(BrainEvent::Err(format!("写入 agent 失败: {e}")));
            return;
        }
        if let Err(e) = self.stdin.as_mut().expect("stdin 存在").write_all(b"\n").await {
            self.kill().await;
            send(BrainEvent::Err(format!("写入 agent 失败: {e}")));
            return;
        }
        if let Err(e) = self.stdin.as_mut().expect("stdin 存在").flush().await {
            self.kill().await;
            send(BrainEvent::Err(format!("写入 agent 失败: {e}")));
            return;
        }

        let timeout = Duration::from_secs(self.cfg.timeout_secs.max(10));
        let deadline = Instant::now() + timeout;
        let mut out = String::new();
        let mut got_output = false;
        // 刚看到 [Tokens] 行(轮结束 = [Tokens] 后的双空行或空 prompt)
        let mut saw_tokens = false;
        // [Tokens] 后的连续空行计数(工具轮中间 [Tokens] 后只有 1 个空行)
        let mut blank_run = 0usize;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            tokio::select! {
                line = self.reader.as_mut().expect("reader 存在").next_line() => {
                    match line {
                        Ok(Some(l)) => match self.dialect.classify(&l) {
                            LineKind::Prompt if got_output => break, // 空 prompt = 轮结束
                            LineKind::Empty => {
                                if saw_tokens && got_output {
                                    blank_run += 1;
                                    if blank_run >= 2 {
                                        break; // [Tokens] 后双空行 = 轮结束
                                    }
                                }
                                continue;
                            }
                            LineKind::Prompt | LineKind::Banner => continue,
                            LineKind::Tokens => {
                                saw_tokens = true;
                                blank_run = 0;
                                continue;
                            }
                            LineKind::ToolCall(tool) => {
                                saw_tokens = false;
                                blank_run = 0;
                                got_output = true;
                                send(BrainEvent::Working(tool));
                            }
                            LineKind::ToolResult => {
                                // 工具结果回显: 不朗读,也不算内容
                                saw_tokens = false;
                                blank_run = 0;
                                continue;
                            }
                            LineKind::Text(t) => {
                                saw_tokens = false;
                                blank_run = 0;
                                got_output = true;
                                out.push_str(&t);
                                out.push(' ');
                                send(BrainEvent::Delta(t));
                            }
                        },
                        Ok(None) => {
                            // 进程退出(可能被外部杀掉)
                            self.kill().await;
                            send(BrainEvent::Err("agent 进程已退出".into()));
                            return;
                        }
                        Err(e) => {
                            self.kill().await;
                            send(BrainEvent::Err(format!("读取 agent 输出失败: {e}")));
                            return;
                        }
                    }
                }
                _ = tokio::time::sleep(remaining) => {
                    self.kill().await;
                    send(BrainEvent::Err(format!(
                        "agent 响应超时(>{}s),已重启",
                        self.cfg.timeout_secs
                    )));
                    return;
                }
            }
        }

        let summary: String = out.trim().chars().take(160).collect();
        send(BrainEvent::Done(if summary.is_empty() {
            "任务完成".into()
        } else {
            summary
        }));
    }

    /// one-shot 协议:每轮把 {prompt} 替换进参数、新起进程,收集 stdout。
    /// 无流式:进程退出后把全文作为一次 Delta + Done(朗读整体退后到完成时)。
    async fn run_one_shot(&mut self, text: &str, tx: flume::Sender<BrainEvent>) {
        let send = |e: BrainEvent| {
            let _ = tx.send(e);
        };

        let command = self.dialect.resolve_command(&self.cfg);
        let args: Vec<String> = self
            .dialect
            .spawn_args(&self.cfg)
            .into_iter()
            .map(|a| a.replace("{prompt}", text))
            .collect();

        let mut cmd = tokio::process::Command::new(&command);
        cmd.args(&args)
            .current_dir(&self.cfg.workdir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                send(BrainEvent::Err(format!("启动 agent {command} 失败: {e}")));
                return;
            }
        };
        let mut stdout = child.stdout.take().expect("agent stdout 可用");
        let mut stderr = child.stderr.take().expect("agent stderr 可用");
        let out_task = tokio::spawn(async move {
            let mut buf = String::new();
            let _ = stdout.read_to_string(&mut buf).await;
            buf
        });
        let err_task = tokio::spawn(async move {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf).await;
            buf
        });

        let timeout = Duration::from_secs(self.cfg.timeout_secs.max(10));
        let ok = match tokio::time::timeout(timeout, child.wait()).await {
            Ok(Ok(s)) => s.success(),
            Ok(Err(e)) => {
                send(BrainEvent::Err(format!("等待 agent {command} 失败: {e}")));
                return;
            }
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                out_task.abort();
                err_task.abort();
                send(BrainEvent::Err(format!(
                    "agent 响应超时(>{}s),已终止",
                    self.cfg.timeout_secs
                )));
                return;
            }
        };

        let stdout_text = out_task.await.unwrap_or_default();
        let stderr_text = err_task.await.unwrap_or_default();
        let reply = stdout_text.trim().to_string();

        if !ok {
            let detail: String = stderr_text.trim().chars().take(300).collect();
            if reply.is_empty() {
                send(BrainEvent::Err(if detail.is_empty() {
                    format!("agent {command} 执行失败(无输出)")
                } else {
                    format!("agent {command} 执行失败: {detail}")
                }));
                return;
            }
            // 退出码非 0 但仍有输出: 播报输出,同时记录错误细节
            tracing::warn!("agent {command} 退出码非 0,stderr: {detail}");
        }

        if reply.is_empty() {
            send(BrainEvent::Done("任务完成".into()));
            return;
        }
        send(BrainEvent::Delta(reply.clone()));
        let summary: String = reply.chars().take(160).collect();
        send(BrainEvent::Done(summary));
    }
}

#[cfg(test)]
mod tests {
    use super::AgentBrain;
    use crate::brain::BrainEvent;
    use crate::config::AgentCfg;

    /// 真实 repl 集成测试:两轮连续对话,验证常驻进程复用
    /// (第二轮应明显快于第一轮,不重复冷启动)。无 jcode 的机器自动跳过。
    #[tokio::test]
    async fn repl_round_trip() {
        let cfg = AgentCfg {
            timeout_secs: 60,
            ..Default::default()
        };
        if !AgentBrain::available(&cfg) {
            eprintln!("SKIP: 未安装 jcode");
            return;
        }
        let mut b = AgentBrain::new(&cfg);
        let mut times = Vec::new();
        for prompt in ["用一句话回答:你好", "用一句话回答:1加1等于几"] {
            let (tx, rx) = flume::unbounded::<BrainEvent>();
            let t0 = std::time::Instant::now();
            b.run_streaming(prompt, tx).await;
            let evs: Vec<_> = rx.drain().collect();
            let elapsed = t0.elapsed().as_secs_f32();
            times.push(elapsed);
            eprintln!("第{}轮耗时 {elapsed:.2}s 事件: {evs:?}", times.len());
            assert!(
                evs.iter().any(|e| matches!(e, BrainEvent::Done(_))),
                "应收到 Done 事件: {evs:?}"
            );
            let reply: String = evs
                .iter()
                .filter_map(|e| match e {
                    BrainEvent::Delta(d) => Some(d.clone()),
                    _ => None,
                })
                .collect();
            assert!(!reply.is_empty(), "应收到增量文本: {evs:?}");
        }
        b.kill().await;
        // 第二轮(进程已就绪)应快于首轮(启动 + 首次模型调用)
        eprintln!("两轮耗时: {times:?}");
        assert!(
            times[1] < times[0] + 3.0,
            "第二轮应复用进程: 首轮 {:.2}s, 第二轮 {:.2}s",
            times[0],
            times[1]
        );
    }

    /// one-shot 协议集成测试:用系统自带 cmd echo 当假 CLI,验证
    /// {prompt} 替换、stdout 全文 → Delta + Done。
    #[cfg(windows)]
    #[tokio::test]
    async fn one_shot_round_trip_with_fake_cli() {
        let cfg = AgentCfg {
            command: "cmd".into(),
            protocol: "one-shot".into(),
            args: vec!["/C".into(), "echo".into(), "{prompt}".into()],
            ..Default::default()
        };
        let mut b = AgentBrain::new(&cfg);
        let (tx, rx) = flume::unbounded::<BrainEvent>();
        b.run_streaming("你好 vox", tx).await;
        let evs: Vec<_> = rx.drain().collect();
        assert!(
            evs.iter()
                .any(|e| matches!(e, BrainEvent::Delta(d) if d.contains("你好 vox"))),
            "应把 stdout 全文作为 Delta: {evs:?}"
        );
        assert!(
            evs.iter().any(|e| matches!(e, BrainEvent::Done(_))),
            "应收到 Done: {evs:?}"
        );
    }

    /// one-shot 协议:CLI 非 0 退出且无输出 → Err 事件(带 stderr 摘要)。
    #[cfg(windows)]
    #[tokio::test]
    async fn one_shot_error_when_cli_exits_nonzero() {
        let cfg = AgentCfg {
            command: "cmd".into(),
            protocol: "one-shot".into(),
            args: vec![
                "/C".into(),
                "echo boom 1>&2 & exit /b 7".into(),
            ],
            ..Default::default()
        };
        let mut b = AgentBrain::new(&cfg);
        let (tx, rx) = flume::unbounded::<BrainEvent>();
        b.run_streaming("随便什么", tx).await;
        let evs: Vec<_> = rx.drain().collect();
        let err = evs
            .iter()
            .find_map(|e| match e {
                BrainEvent::Err(m) => Some(m.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("应收到 Err: {evs:?}"));
        assert!(err.contains("boom"), "Err 应带 stderr 摘要: {err}");
    }
}
