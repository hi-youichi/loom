//! Agent runner implementations

use async_trait::async_trait;
use teloxide::Bot;
use crate::traits::AgentRunner;
use crate::error::BotError;
use crate::config::Settings;

pub struct LoomAgentRunner {
    bot: Bot,
    settings: Settings,
}

impl LoomAgentRunner {
    pub fn new(bot: Bot, settings: Settings) -> Self {
        Self { bot, settings }
    }
}

#[async_trait]
impl AgentRunner for LoomAgentRunner {
    async fn run(
        &self,
        prompt: &str,
        chat_id: i64,
        message_id: Option<i32>,
    ) -> Result<String, BotError> {
        crate::streaming::run_loom_agent_streaming(
            prompt,
            chat_id,
            self.bot.clone(),
            message_id,
            &self.settings,
        )
        .await
        .map_err(|e| BotError::Agent(e.to_string()))
    }
}
