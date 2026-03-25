//! Handler dependency container
//!
//! Groups all dependencies needed by the message handler.

use std::collections::HashSet;
use std::sync::Arc;

use teloxide::Bot;
use tokio::sync::Mutex;

use crate::agent::LoomAgentRunner;
use crate::config::Settings;
use crate::download::TeloxideDownloader;
use crate::sender::TeloxideSender;
use crate::session::SqliteSessionManager;
use crate::traits::{AgentRunner, FileDownloader, MessageSender, SessionManager};

#[derive(Default)]
pub struct ChatRunRegistry {
    active_chats: Mutex<HashSet<i64>>,
}

impl ChatRunRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn try_acquire(self: &Arc<Self>, chat_id: i64) -> Option<ChatRunGuard> {
        let mut active = self.active_chats.lock().await;
        if !active.insert(chat_id) {
            return None;
        }
        Some(ChatRunGuard {
            registry: Arc::clone(self),
            chat_id,
        })
    }

    pub async fn release_chat(&self, chat_id: i64) {
        self.active_chats.lock().await.remove(&chat_id);
    }
}

pub struct ChatRunGuard {
    registry: Arc<ChatRunRegistry>,
    chat_id: i64,
}

impl ChatRunGuard {
    /// Releases the chat slot before returning from the handler so the next message is not
    /// spuriously rejected as busy while a spawned task would still hold the lock.
    pub async fn release(self) {
        self.registry.release_chat(self.chat_id).await;
    }
}

/// Handler dependencies
pub struct HandlerDeps {
    pub sender: Arc<dyn MessageSender>,
    pub agent: Arc<dyn AgentRunner>,
    pub session: Arc<dyn SessionManager>,
    pub downloader: Arc<dyn FileDownloader>,
    pub settings: Arc<Settings>,
    pub bot_username: Arc<String>,
    pub run_registry: Arc<ChatRunRegistry>,
}

impl HandlerDeps {
    /// Production stack for one bot after `get_me` has filled `bot_username`.
    pub fn production(
        bot: Bot,
        settings: Arc<Settings>,
        bot_username: Arc<String>,
        run_registry: Arc<ChatRunRegistry>,
    ) -> Self {
        let download_dir = settings.download_dir.clone();
        Self {
            sender: Arc::new(TeloxideSender::new(bot.clone())),
            agent: Arc::new(LoomAgentRunner::new(bot.clone(), (*settings).clone())),
            session: Arc::new(SqliteSessionManager::new()),
            downloader: Arc::new(TeloxideDownloader::new(bot, download_dir)),
            settings,
            bot_username,
            run_registry,
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
        run_registry: Arc<ChatRunRegistry>,
    ) -> Self {
        Self {
            sender,
            agent,
            session,
            downloader,
            settings,
            bot_username,
            run_registry,
        }
    }
}
