//! one-shot 适配器:面向只提供"一次性任务 + stdout 结果"的 CLI。
//! 每轮请求把 {prompt} 占位符替换进启动参数、新起一个进程,进程打印最终
//! 回复文本后退出。无流式增量、无工具事件、无跨轮进程复用。
//!
//! 适用:
//! - DeepSeek Harness: dsh --profile headless "{prompt}"
//! - 其他一次性 agent CLI(claude -p、gemini、codex 等),只要能 stdout 出结果
//!
//! 代价(相对 jcode repl):每轮付 CLI 冷启动;回复只能等进程退出后整体朗读;
//! 没有 [工具] 事件(无"干活"动画);跨轮记忆需靠上层在 prompt 里携带历史。

use super::{CliDialect, LineKind};
use crate::config::AgentCfg;

/// one-shot 方言:每轮新进程,stdout 全文 = 最终回复。
pub struct OneShotDialect;

impl CliDialect for OneShotDialect {
    /// Windows:npm 全局安装的 CLI(如 dsh)在 .bin 目录里同时存在无扩展名
    /// bash shim 与 .cmd 版本,CreateProcess 命中无扩展名 shim 会执行失败,
    /// 因此把 .cmd 变体也放进候选探测(见 CliDialect::resolve_command)。
    fn probe_commands(&self, cfg: &AgentCfg) -> Vec<String> {
        let mut cmds = vec![cfg.command.clone()];
        #[cfg(windows)]
        {
            let lower = cfg.command.to_lowercase();
            if !lower.ends_with(".exe") && !lower.ends_with(".cmd") {
                cmds.push(format!("{}.cmd", cfg.command));
            }
        }
        cmds
    }

    /// 启动参数模板;{prompt} 会被替换为用户输入。未配置时默认把输入
    /// 作为唯一参数。
    fn spawn_args(&self, cfg: &AgentCfg) -> Vec<String> {
        if cfg.args.is_empty() {
            vec!["{prompt}".into()]
        } else {
            cfg.args.clone()
        }
    }

    /// one-shot 不走行分类。
    fn classify(&self, line: &str) -> LineKind {
        LineKind::Text(line.to_string())
    }

    fn persistent(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::OneShotDialect;
    use crate::brain::adapters::CliDialect;
    use crate::config::AgentCfg;

    #[test]
    fn defaults_to_prompt_as_single_arg() {
        let cfg = AgentCfg::default();
        assert_eq!(OneShotDialect.spawn_args(&cfg), vec!["{prompt}"]);
    }

    /// Windows 上候选探测应包含 .cmd 变体(npm shim 坑)。
    #[cfg(windows)]
    #[test]
    fn probe_includes_cmd_shim_on_windows() {
        let cfg = AgentCfg {
            command: "dsh".into(),
            ..Default::default()
        };
        assert_eq!(OneShotDialect.probe_commands(&cfg), vec!["dsh", "dsh.cmd"]);
        // 已带扩展名的配置不再追加
        let cfg = AgentCfg {
            command: "dsh.cmd".into(),
            ..Default::default()
        };
        assert_eq!(OneShotDialect.probe_commands(&cfg), vec!["dsh.cmd"]);
    }

    #[test]
    fn uses_configured_arg_template() {
        let cfg = AgentCfg {
            protocol: "one-shot".into(),
            command: "dsh".into(),
            args: vec!["--profile".into(), "headless".into(), "{prompt}".into()],
            ..Default::default()
        };
        assert_eq!(
            OneShotDialect.spawn_args(&cfg),
            vec!["--profile", "headless", "{prompt}"]
        );
        assert!(!OneShotDialect.persistent());
    }
}
