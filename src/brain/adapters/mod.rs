//! CLI agent 适配器:把"某个命令行 coding agent 的启动方式 + 输出方言"
//! 封装成统一 trait。AgentBrain 只负责进程生命周期(启动/重启/超时/行循环),
//! 接新 CLI(DSH / claude / gemini / codex / 自制工具)只需新增一个适配器,
//! 或在 config.toml 里切换 protocol。

mod jcode;
mod one_shot;

use crate::config::AgentCfg;

/// repl 输出行分类:把各 CLI 的方言行归一成统一语义。
#[derive(Debug, Clone, PartialEq)]
pub enum LineKind {
    /// 启动 banner(标题/提示/技能列表)
    Banner,
    /// [Tokens] upload: ... 类元信息
    Tokens,
    /// [工具名] 参数 → agent 正在调用工具
    ToolCall(String),
    /// 工具结果回显(不朗读)
    ToolResult,
    /// 空 prompt(等待输入 / 本轮结束标记)
    Prompt,
    /// 空行
    Empty,
    /// 正文行 → 流式文本
    Text(String),
}

/// CLI 适配器:一种命令行 agent 的方言(探测、启动参数、输出解析、驻留方式)。
pub trait CliDialect: Send + Sync {
    /// 候选探测命令:顺序探测,第一个 --version 成功即视为已安装。
    fn probe_commands(&self, cfg: &AgentCfg) -> Vec<String>;

    /// 解析实际要 spawn 的可执行文件(argv[0])。默认取第一个探测成功的
    /// 候选命令,探测全失败则回退配置的 command(启动时报错)。
    ///
    /// 必须覆盖的情形:Windows 上 npm 全局安装的 CLI(如 dsh)在 .bin 目录里
    /// 同时存在无扩展名 bash shim / .cmd / .ps1 三个文件,CreateProcess 按
    /// PATH 搜索会命中无扩展名 shim 而执行失败;适配器应把 .cmd 版本放进
    /// 候选列表,由本方法选出真正可执行的条目。
    fn resolve_command(&self, cfg: &AgentCfg) -> String {
        self.probe_commands(cfg)
            .into_iter()
            .find(|c| cmd_available(c))
            .unwrap_or_else(|| cfg.command.clone())
    }

    /// spawn 参数(argv[0] 之后)。one-shot 协议里 {prompt} 占位符
    /// 会被替换为用户输入。
    fn spawn_args(&self, cfg: &AgentCfg) -> Vec<String>;

    /// repl 输出行分类。one-shot 协议不走行分类,默认实现返回正文行。
    fn classify(&self, _line: &str) -> LineKind {
        LineKind::Text(String::new())
    }

    /// true = 常驻 repl(进程复用/流式增量/工具事件);false = one-shot
    /// (每轮新起进程,stdout 全文为最终回复,无流式)。
    fn persistent(&self) -> bool {
        true
    }

    /// 探测 CLI 是否可用(未安装则上层自动降级 DeepSeek)。
    fn available(&self, cfg: &AgentCfg) -> bool {
        self.probe_commands(cfg).iter().any(|c| cmd_available(c))
    }
}

/// 按配置选择适配器。默认 jcode(向后兼容旧配置);
/// protocol = "one-shot" 走一次性 CLI(DSH 等)。
pub fn from_config(cfg: &AgentCfg) -> Box<dyn CliDialect> {
    match cfg.protocol.as_str() {
        "one-shot" => Box::new(one_shot::OneShotDialect),
        _ => Box::new(jcode::JcodeDialect),
    }
}

/// 探测命令是否可执行(非交互,stdout/stderr 丢弃)。
pub(crate) fn cmd_available(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
