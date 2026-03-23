//! Handler dependency container
//!
//! Groups all dependencies needed by the message handler

use std::sync::Arc;
use crate::config::Settings;
use crate::traits::{MessageSender, AgentRunner, SessionManager, FileDownloader};
use crate::error::BotError;

/// Handler dependencies
pub struct HandlerDeps {
    pub sender: Arc<dyn MessageSender>,
    pub agent: Arc<dyn AgentRunner>,
    pub session: Arc<dyn SessionManager>,
    pub downloader: Arc<dyn FileDownloader>,
    pub settings: Arc<Settings>,
    pub bot_username: Arc<String>,
}

impl HandlerDeps {
    /// Create production dependencies
    pub fn production(
        bot: teloxide::Bot,
        settings: Settings,
    ) -> Self {
        Self {
            sender: Arc::new(TeloxideSender::new(bot)),
            agent: Arc::new(LoomAgentRunner::new(bot.clone(), settings)),
            session: Arc::new(SqliteSessionManager::new()),
            downloader: Arc::new(TeloxideDownloader::new(bot.clone(), settings.streaming)),
            bot_username: Arc::new(String::new()),
        }
    }
}

}

