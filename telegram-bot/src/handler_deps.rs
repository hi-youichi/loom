//! Handler dependency container
//!
//! Groups all dependencies needed by the message handler.

use std::sync::Arc;

use teloxide::Bot;

use crate::agent::LoomAgentRunner;
use crate::config::Settings;
use crate::download::TeloxideDownloader;
use crate::sender::TeloxideSender;
use crate::session::SqliteSessionManager;
use crate::traits::{AgentRunner, FileDownloader, MessageSender, SessionManager};

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
    /// Production stack for one bot after `get_me` has filled `bot_username`.
    pub fn production(bot: Bot, settings: Arc<Settings>, bot_username: Arc<String>) -> Self {
        let download_dir = settings.download_dir.clone();
        Self {
            sender: Arc::new(TeloxideSender::new(bot.clone())),
            agent: Arc::new(LoomAgentRunner::new(bot.clone(), (*settings).clone())),
            session: Arc::new(SqliteSessionManager::new()),
            downloader: Arc::new(TeloxideDownloader::new(bot, download_dir)),
            settings,
            bot_username,
        }
    }

    /// Test stack with explicit doubles (for example [`crate::mock`] types).
    pub fn for_test(
        sender: Arc<dyn MessageSender>,
        agent: Arc<dyn AgentRunner>,
        session: Arc<dyn SessionManager>,
        downloader: Arc<dyn FileDownloader>,
        settings: Arc<Settings>,
        bot_username: Arc<String>,
    ) -> Self {
        Self {
            sender,
            agent,
            session,
            downloader,
            settings,
            bot_username,
        }
    }
}
