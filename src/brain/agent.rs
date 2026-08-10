use tokio::io::{AsyncBufReadExt, BufReader};

use super::BrainEvent;
use crate::config::AgentCfg;

/// Agent 大脑:把用户语音作为 prompt 喂给 `jcode run` / `pi` 子进程,
/// stdout 按行流式转发为 Delta 事件。
/// 实验性:输出格式取决于 agent 的 CLI 行为,后续可接结构化输出。
pub struct AgentBrain {
    cfg: AgentCfg,
}

impl AgentBrain {
    pub fn new(cfg: &AgentCfg) -> Self {
        Self { cfg: cfg.clone() }
    }

    pub async fn run(&mut self, text: &str) -> Vec<BrainEvent> {
        let mut events = Vec::new();
        let mut child = match tokio::process::Command::new(&self.cfg.command)
            .arg("run")
            .arg(text)
            .current_dir(&self.cfg.workdir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                return vec![BrainEvent::Err(format!(
                    "启动 agent {} 失败: {e}",
                    self.cfg.command
                ))]
            }
        };

        let stdout = child.stdout.take().expect("stdout piped");
        let mut lines = BufReader::new(stdout).lines();
        let mut out = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            out.push_str(&line);
            out.push(' ');
            events.push(BrainEvent::Delta(line));
        }
        let _ = child.wait().await;

        let summary: String = out.trim().chars().take(160).collect();
        events.push(BrainEvent::Done(if summary.is_empty() {
            "任务完成".into()
        } else {
            summary
        }));
        events
    }
}
