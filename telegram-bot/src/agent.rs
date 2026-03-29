use std::sync::Arc;

use async_trait::async_trait;
use teloxide::Bot;

use crate::config::Settings;
use crate::error::BotError;
use crate::sender::TeloxideSender;
use crate::traits::{AgentRunContext, AgentRunner, MessageSender};

pub struct LoomAgentRunner {
    bot: Bot,
    settings: Arc<Settings>,
}

impl LoomAgentRunner {
    pub fn new(bot: Bot, settings: Arc<Settings>) -> Self {
        Self { bot, settings }
    }
}

#[async_trait]
impl AgentRunner for LoomAgentRunner {
    async fn run(
        &self,
        prompt: &str,
        chat_id: i64,
        context: AgentRunContext,
    ) -> Result<String, BotError> {
        let sender: Arc<dyn MessageSender> = Arc::new(TeloxideSender::new(self.bot.clone()));
        crate::streaming::run_loom_agent_streaming(
            prompt,
            chat_id,
            sender,
            context,
            &self.settings,
        )
        .await
        .map_err(|e| BotError::Agent(e.to_string()))
    }
}
