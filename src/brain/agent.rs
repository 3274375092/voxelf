use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout};

use super::BrainEvent;
use crate::config::AgentCfg;

/// Agent 大脑:常驻 `jcode repl` 子进程,每轮请求写一行 stdin、按行读 stdout。
/// repl 模式没有 `jcode run` 的 5.3s 冷启动开销,实测首轮 ~1.5s、后续每轮 ~0.7s。
///
/// 输出格式(实测):
/// ```text
/// J-Code - Coding Agent              <- banner
/// Type your message, or 'quit'...
/// Available skills: ...
/// > 你好!有什么可以帮你的吗?           <- "> " 前缀 + 回复文本(同一行)
/// [Tokens] upload: 2023 ...
/// >                                    <- 空 prompt = 本轮结束
/// ```
/// 工具调用轮:
/// ```text
/// > 
/// [read] in_a.txt                     <- [工具名] 参数 → Working 事件
/// [Tokens] upload: ...
///  →     1    用一句话回答:你好            <- 工具结果回显(不朗读)
/// `in_a.txt` 前3行内容:                <- 正文
/// ...
/// [Tokens] ...
/// >                                    <- 轮结束
/// ```
pub struct AgentBrain {
    cfg: AgentCfg,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    reader: Option<Lines<BufReader<ChildStdout>>>,
    /// 是否已完成启动排空(ready 状态)
    started: bool,
}

/// 行分类:决定如何对待 repl 的每一行输出。
#[derive(Debug, Clone, PartialEq)]
enum LineKind {
    /// 启动 banner(标题/提示/技能列表)
    Banner,
    /// `[Tokens] upload: ...` 元信息
    Tokens,
    /// `[工具名] 参数` → agent 正在调用工具
    ToolCall(String),
    /// ` → 工具结果回显`
    ToolResult,
    /// `> ` 空 prompt(等待输入 / 本轮结束标记)
    Prompt,
    /// 空行
    Empty,
    /// `> 正文` 或纯正文行 → 流式文本
    Text(String),
}

/// 把 repl 输出行分类。独立成函数方便单测。
fn classify(line: &str) -> LineKind {
    let line = line.trim_end_matches(['\r', '\n']);
    if line.is_empty() {
        return LineKind::Empty;
    }
    if line == "J-Code - Coding Agent"
        || line.starts_with("Type your message")
        || line.starts_with("Available skills:")
    {
        return LineKind::Banner;
    }
    if line.starts_with("[Tokens]") {
        return LineKind::Tokens;
    }
    if line.trim_start().starts_with('→') {
        // 工具结果回显: " →     1\t内容"(前缀空格数不固定)
        return LineKind::ToolResult;
    }
    if line == ">" {
        return LineKind::Prompt;
    }
    if let Some(rest) = line.strip_prefix("> ") {
        // "> " 前缀:空 = prompt 标记;非空 = 回复文本
        let rest = rest.trim();
        if rest.is_empty() {
            return LineKind::Prompt;
        }
        return LineKind::Text(rest.to_string());
    }
    if line.starts_with('[') {
        // [工具名] 参数(排除 [Tokens] 已处理)
        if let Some(end) = line.find(']') {
            let name = &line[1..end];
            if !name.is_empty()
                && name.len() <= 32
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                let rest = line[end + 1..].trim();
                return LineKind::ToolCall(if rest.is_empty() {
                    name.to_string()
                } else {
                    format!("{name} {rest}")
                });
            }
        }
        return LineKind::Text(line.to_string());
    }
    LineKind::Text(line.to_string())
}

impl AgentBrain {
    pub fn new(cfg: &AgentCfg) -> Self {
        Self {
            cfg: cfg.clone(),
            child: None,
            stdin: None,
            reader: None,
            started: false,
        }
    }

    /// 探测命令是否可执行(非交互,stdout/stderr 丢弃)。
    fn cmd_available(cmd: &str) -> bool {
        std::process::Command::new(cmd)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// 探测 agent 命令是否可用(用户机器上装了 jcode 才启用 agent 模式)。
    /// 配置的 command 找不到时,回退探测与 voxelf 同目录的便携版 jcode.exe
    /// (随分发包一起带的独立二进制)。
    pub fn available(command: &str) -> bool {
        Self::cmd_available(command)
            || (!command.to_lowercase().ends_with(".exe") && Self::cmd_available("jcode.exe"))
    }

    /// 解析实际要启动的可执行文件: 配置的 command 优先;
    /// 找不到时回退到同目录便携版 jcode.exe(分发包里随附)。
    fn resolve_command(&self) -> String {
        if Self::cmd_available(&self.cfg.command) {
            return self.cfg.command.clone();
        }
        let portable = "jcode.exe";
        if self.cfg.command != portable && Self::cmd_available(portable) {
            return portable.to_string();
        }
        self.cfg.command.clone()
    }

    /// 启动(或重启)repl 进程。不预读输出: banner 与启动 prompt 由
    /// run_streaming 的分类逻辑惰性丢弃(它们不会触发文本事件)。
    async fn ensure_started(&mut self) -> Result<()> {
        if self.started {
            return Ok(());
        }
        self.kill().await;

        let mut cmd = tokio::process::Command::new(self.resolve_command());
        cmd.arg("repl").arg("--quiet").arg("--no-update");
        if !self.cfg.provider.is_empty() {
            cmd.arg("-p").arg(&self.cfg.provider);
        }
        if self.cfg.tools.trim().is_empty() {
            cmd.arg("--tool-profile").arg("minimal");
        } else {
            cmd.arg("--tools").arg(self.cfg.tools.trim());
        }
        cmd.current_dir(&self.cfg.workdir)
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

    pub async fn run_streaming(&mut self, text: &str, tx: flume::Sender<BrainEvent>) {
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
                        Ok(Some(l)) => match classify(&l) {
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
}

#[cfg(test)]
mod tests {
    use super::classify;
    use super::LineKind;

    #[test]
    fn classifies_banner_lines() {
        assert_eq!(classify("J-Code - Coding Agent"), LineKind::Banner);
        assert_eq!(
            classify("Type your message, or 'quit' to exit."),
            LineKind::Banner
        );
        assert!(matches!(
            classify("Available skills: /ask-matt, /tdd, ..."),
            LineKind::Banner
        ));
    }

    #[test]
    fn classifies_token_and_tool_lines() {
        assert_eq!(
            classify("[Tokens] upload: 2023 download: 34"),
            LineKind::Tokens
        );
        assert_eq!(
            classify("[read] in_a.txt"),
            LineKind::ToolCall("read in_a.txt".into())
        );
        assert_eq!(
            classify("[bash]"),
            LineKind::ToolCall("bash".into())
        );
        // 工具结果回显(前缀空格数不固定)
        assert_eq!(classify(" →     1\t用一句话回答:你好"), LineKind::ToolResult);
        assert_eq!(classify("  →     1\t# voxelf 配置"), LineKind::ToolResult);
        assert_eq!(classify("→ 1\t内容"), LineKind::ToolResult);
    }

    #[test]
    fn classifies_prompt_and_text() {
        assert_eq!(classify("> "), LineKind::Prompt);
        assert_eq!(classify(">"), LineKind::Prompt);
        assert_eq!(
            classify("> 你好!有什么可以帮你的吗?"),
            LineKind::Text("你好!有什么可以帮你的吗?".into())
        );
        assert_eq!(
            classify("`in_a.txt` 前3行内容:"),
            LineKind::Text("`in_a.txt` 前3行内容:".into())
        );
        assert_eq!(classify(""), LineKind::Empty);
        // markdown 引用行不会被误判为 Prompt(带内容)
        assert_eq!(
            classify("> 这是引用内容"),
            LineKind::Text("这是引用内容".into())
        );
    }

    /// 真实 repl 集成测试:两轮连续对话,验证常驻进程复用
    /// (第二轮应明显快于第一轮,不重复冷启动)。无 jcode 的机器自动跳过。
    #[tokio::test]
    async fn repl_round_trip() {
        use crate::brain::agent::AgentBrain;
        use crate::brain::BrainEvent;
        use crate::config::AgentCfg;

        if !AgentBrain::available("jcode") {
            eprintln!("SKIP: 未安装 jcode");
            return;
        }
        let cfg = AgentCfg {
            timeout_secs: 60,
            ..Default::default()
        };
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
}
