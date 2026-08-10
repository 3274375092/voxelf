use super::{agent::AgentBrain, deepseek::DeepSeekBrain, BrainEvent};
use crate::config::Config;

/// 双层大脑:闲聊走 DeepSeek(首字 0.5s),操作类请求转常驻 jcode agent(首轮 ~1.5s)。
/// jcode 未安装或禁用时自动降级为纯 DeepSeek。
pub struct HybridBrain {
    deepseek: DeepSeekBrain,
    agent: Option<AgentBrain>,
}

/// 意图分类结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// 闲聊/问答:DeepSeek 足够
    Chat,
    /// 操作类:需要 agent 执行工具
    Agent,
}

/// 强信号前缀:命中即视为操作请求(对电脑/文件/程序的明确动作)。
/// 刻意不含"帮我看看/帮我查一下/帮我找"(信息查询,DeepSeek 能答)。
const STRONG_PREFIX: &[&str] = &[
    "帮我做",
    "帮我写",
    "帮我创建",
    "帮我删除",
    "帮我整理",
    "帮我下载",
    "帮我打开",
    "帮我运行",
    "帮我执行",
    "帮我安装",
    "帮我启动",
    "帮我关掉",
    "帮我关闭",
    "帮我把",
    "帮我读",
    // 查询类(需要联网,DeepSeek 不能联网会编造): 搜索/查天气/查新闻/看网页
    "帮我查",
    "帮我搜",
    "帮我看看",
    "帮我查一下",
    "帮我搜一下",
];

/// 动作词:与 OBJECTS 组合命中才算操作(单独出现不算,如"打开音乐")。
const ACTIONS: &[&str] = &[
    "创建", "删除", "移动", "复制", "重命名", "下载", "安装", "运行", "执行", "打开", "关闭", "整理", "搜索",
    "查找", "修改", "编辑", "写入", "生成", "压缩", "解压", "读取",
];

/// 对象词:电脑/文件域名词。
const OBJECTS: &[&str] = &[
    "文件", "文件夹", "目录", "程序", "软件", "脚本", "项目", "代码", "命令", "配置", "桌面", "文档", "图片",
    "视频", "压缩包", "数据库", "服务", "进程", "工具",
];

/// 意图判断(纯规则,零延迟零成本)。保守设计:宁可误判为闲聊(DeepSeek 兜底),
/// 不把日常聊天塞给 agent。
pub fn classify_intent(text: &str) -> Intent {
    for p in STRONG_PREFIX {
        if text.contains(p) {
            return Intent::Agent;
        }
    }
    let has_action = ACTIONS.iter().any(|a| text.contains(a));
    let has_object = OBJECTS.iter().any(|o| text.contains(o));
    if has_action && has_object {
        return Intent::Agent;
    }
    Intent::Chat
}

impl HybridBrain {
    pub fn new(cfg: &Config) -> Self {
        let deepseek = DeepSeekBrain::new(&cfg.deepseek);
        // jcode 可用才启用 agent;否则整个 hybrid 退化为纯 DeepSeek
        let agent = if cfg.brain.agent.enabled && AgentBrain::available(&cfg.brain.agent.command) {
            Some(AgentBrain::new(&cfg.brain.agent))
        } else {
            None
        };
        if agent.is_none() {
            tracing::warn!(
                "agent 不可用(enabled={}, command={}), 降级为纯 DeepSeek",
                cfg.brain.agent.enabled,
                cfg.brain.agent.command
            );
        }
        Self { deepseek, agent }
    }

    pub async fn run_streaming(&mut self, text: &str, tx: flume::Sender<BrainEvent>) {
        if matches!(classify_intent(text), Intent::Agent)
            && let Some(agent) = &mut self.agent
        {
            tracing::info!("[hybrid] 操作类请求 → agent: {text}");
            let _ = tx.send(BrainEvent::Working("任务".into()));
            // 轻量提示: 缓解模型偶发不调用 web 工具(deepseek-v4-flash 曾直接拒绝查天气)
            let boosted = format!(
                "{text}\n(若需要天气/新闻/实时等最新信息,请使用 websearch 或 webfetch 工具获取真实数据,不要凭空回答)"
            );
            agent.run_streaming(&boosted, tx).await;
            return;
        }
        tracing::info!("[hybrid] 闲聊 → DeepSeek: {text}");
        self.deepseek.run_streaming(text, tx).await;
    }
}

#[cfg(test)]
mod tests {
    use super::classify_intent;
    use super::Intent;

    #[test]
    fn chat_utterances_stay_chat() {
        for s in [
            "你好呀",
            "今天天气怎么样",
            "明天会下雨吗",
            "给我讲个笑话",
            "你会唱什么歌",
            "打开音乐播放器", // 动作但无对象词,不误判
            "我们明天去哪玩",
        ] {
            assert_eq!(classify_intent(s), Intent::Chat, "应保持闲聊: {s}");
        }
    }

    #[test]
    fn agent_utterances_are_agent() {
        for s in [
            "帮我写一个 python 脚本",
            "帮我把桌面上的文件整理一下",
            "删除这个文件夹",
            "帮我下载一个软件",
            "帮我运行刚才那个脚本",
            "帮我创建新项目",
            "帮我安装 git",
            "帮我关闭正在运行的进程",
            "帮我把下载的文件移动到桌面",
            "帮我打开文件管理器",
            "帮我读一下 config.toml 的前5行",
            "读取这个配置文件",
            // 联网查询(DeepSeek 不联网,转 agent 用 websearch/webfetch)
            "帮我看看明天的天气",
            "帮我查一下北京到上海的高铁",
            "帮我搜一下最近的新闻",
            "帮我查一下这个词是什么意思",
        ] {
            assert_eq!(classify_intent(s), Intent::Agent, "应判定为操作: {s}");
        }
    }
}
