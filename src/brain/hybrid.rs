use super::{agent::AgentBrain, deepseek::DeepSeekBrain, BrainEvent};
use crate::config::Config;

/// 双层大脑(agent-first):检测到 jcode 时**所有**请求都走常驻 agent
/// (带 Vox 人设,工具/联网/上下文记忆全具备);未安装或禁用时自动降级纯 DeepSeek。
pub struct HybridBrain {
    deepseek: DeepSeekBrain,
    agent: Option<AgentBrain>,
}

/// Vox 人设注入:让 agent 的输出与 DeepSeek 直连保持一致的口语化风格
/// (语音朗读友好: 无列表/markdown/emoji,句号分隔)。
/// 每轮都带上(重复指令模型会忽略,但保证 agent 重启后不丢人设)。
fn persona_prompt(text: &str) -> String {
    format!(
        "你是 Vox,一个住在像素世界里的可爱小精灵伙伴。用简短、口语化的中文回复,语气活泼、温暖、有点俏皮。\
         回复会被语音合成朗读出来,不要使用列表、markdown 符号、emoji 或表情符号;句子之间用句号或感叹号分隔。\
         如果需要天气、新闻、实时等最新信息,使用 websearch 或 webfetch 工具获取真实数据,不要凭空回答。\
         需要操作文件或执行任务时直接调用工具完成,完成后用一句话口语化汇报结果。\
         用户说: {text}"
    )
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
        if let Some(agent) = &mut self.agent {
            tracing::info!("[hybrid] agent-first → jcode: {text}");
            // 不预先发 Working: 闲聊保持"思考"动画,只有真实工具调用才切"干活"
            agent.run_streaming(&persona_prompt(text), tx).await;
            return;
        }
        tracing::info!("[hybrid] 降级 → DeepSeek: {text}");
        self.deepseek.run_streaming(text, tx).await;
    }
}

#[cfg(test)]
mod tests {
    use super::persona_prompt;
    use crate::brain::hybrid::HybridBrain;
    use crate::config::Config;

    #[test]
    fn persona_injection_keeps_request_and_rules() {
        let p = persona_prompt("今天天气怎么样");
        assert!(p.contains("Vox"), "应有人设");
        assert!(p.contains("websearch"), "应有联网工具指令");
        assert!(p.contains("今天天气怎么样"), "用户请求应完整保留");
        assert!(p.contains("句号或感叹号"), "应有朗读友好规则");
    }

    #[test]
    fn disabled_agent_falls_back_to_deepseek() {
        let mut cfg = Config::default();
        cfg.brain.agent.enabled = false;
        let h = HybridBrain::new(&cfg);
        assert!(h.agent.is_none(), "enabled=false 时不应启用 agent");
    }
}
