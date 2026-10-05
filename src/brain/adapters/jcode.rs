//! jcode 适配器:jcode repl 的启动参数与输出方言(从旧 AgentBrain 原样迁移)。
//!
//! 输出格式(实测):
//! ```text
//! J-Code - Coding Agent              <- banner
//! Type your message, or 'quit'...
//! Available skills: ...
//! > 你好!有什么可以帮你的吗?           <- "> " 前缀 + 回复文本(同一行)
//! [Tokens] upload: 2023 ...
//! >                                    <- 空 prompt = 本轮结束
//! ```
//! 工具调用轮:
//! ```text
//! >
//! [read] in_a.txt                     <- [工具名] 参数 → Working 事件
//! [Tokens] upload: ...
//!  →     1    用一句话回答:你好            <- 工具结果回显(不朗读)
//! `in_a.txt` 前3行内容:                <- 正文
//! ...
//! [Tokens] ...
//! >                                    <- 轮结束
//! ```

use super::{CliDialect, LineKind};
use crate::config::AgentCfg;

/// jcode 方言:常驻 `jcode repl` 进程,每轮写一行 stdin、按行读 stdout。
/// repl 模式没有 `jcode run` 的 5.3s 冷启动开销,实测首轮 ~1.5s、后续 ~0.7s。
pub struct JcodeDialect;

impl CliDialect for JcodeDialect {
    /// 配置的 command 找不到时,回退探测与 voxelf 同目录的便携版 jcode.exe
    /// (随分发包一起带的独立二进制)。
    fn probe_commands(&self, cfg: &AgentCfg) -> Vec<String> {
        let mut cmds = vec![cfg.command.clone()];
        if !cfg.command.to_lowercase().ends_with(".exe") {
            cmds.push("jcode.exe".into());
        }
        cmds
    }

    fn spawn_args(&self, cfg: &AgentCfg) -> Vec<String> {
        let mut args = vec!["repl".into(), "--quiet".into(), "--no-update".into()];
        if !cfg.provider.is_empty() {
            args.push("-p".into());
            args.push(cfg.provider.clone());
        }
        if cfg.tools.trim().is_empty() {
            args.push("--tool-profile".into());
            args.push("minimal".into());
        } else {
            args.push("--tools".into());
            args.push(cfg.tools.trim().into());
        }
        args
    }

    fn classify(&self, line: &str) -> LineKind {
        classify_jcode(line)
    }
}

/// 把 jcode repl 输出行分类。独立成函数方便单测。
pub(crate) fn classify_jcode(line: &str) -> LineKind {
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

#[cfg(test)]
mod tests {
    use super::{classify_jcode, JcodeDialect};
    use crate::brain::adapters::{CliDialect, LineKind};
    use crate::config::AgentCfg;

    #[test]
    fn classifies_banner_lines() {
        assert_eq!(classify_jcode("J-Code - Coding Agent"), LineKind::Banner);
        assert_eq!(
            classify_jcode("Type your message, or 'quit' to exit."),
            LineKind::Banner
        );
        assert!(matches!(
            classify_jcode("Available skills: /ask-matt, /tdd, ..."),
            LineKind::Banner
        ));
    }

    #[test]
    fn classifies_token_and_tool_lines() {
        assert_eq!(
            classify_jcode("[Tokens] upload: 2023 download: 34"),
            LineKind::Tokens
        );
        assert_eq!(
            classify_jcode("[read] in_a.txt"),
            LineKind::ToolCall("read in_a.txt".into())
        );
        assert_eq!(
            classify_jcode("[bash]"),
            LineKind::ToolCall("bash".into())
        );
        // 工具结果回显(前缀空格数不固定)
        assert_eq!(classify_jcode(" →     1\t用一句话回答:你好"), LineKind::ToolResult);
        assert_eq!(classify_jcode("  →     1\t# voxelf 配置"), LineKind::ToolResult);
        assert_eq!(classify_jcode("→ 1\t内容"), LineKind::ToolResult);
    }

    #[test]
    fn classifies_prompt_and_text() {
        assert_eq!(classify_jcode("> "), LineKind::Prompt);
        assert_eq!(classify_jcode(">"), LineKind::Prompt);
        assert_eq!(
            classify_jcode("> 你好!有什么可以帮你的吗?"),
            LineKind::Text("你好!有什么可以帮你的吗?".into())
        );
        assert_eq!(
            classify_jcode("`in_a.txt` 前3行内容:"),
            LineKind::Text("`in_a.txt` 前3行内容:".into())
        );
        assert_eq!(classify_jcode(""), LineKind::Empty);
        // markdown 引用行不会被误判为 Prompt(带内容)
        assert_eq!(
            classify_jcode("> 这是引用内容"),
            LineKind::Text("这是引用内容".into())
        );
    }

    #[test]
    fn spawn_args_cover_provider_and_tools() {
        let cfg = AgentCfg::default();
        let args = JcodeDialect.spawn_args(&cfg);
        assert_eq!(args, vec!["repl", "--quiet", "--no-update", "-p", "deepseek", "--tool-profile", "minimal"]);
        let cfg = AgentCfg {
            provider: "".into(),
            tools: "read,bash".into(),
            ..Default::default()
        };
        let args = JcodeDialect.spawn_args(&cfg);
        assert_eq!(args, vec!["repl", "--quiet", "--no-update", "--tools", "read,bash"]);
    }

    #[test]
    fn probe_falls_back_to_portable_exe() {
        let cfg = AgentCfg::default();
        assert_eq!(
            JcodeDialect.probe_commands(&cfg),
            vec!["jcode", "jcode.exe"]
        );
        let cfg = AgentCfg {
            command: "jcode.exe".into(),
            ..Default::default()
        };
        assert_eq!(JcodeDialect.probe_commands(&cfg), vec!["jcode.exe"]);
    }
}
