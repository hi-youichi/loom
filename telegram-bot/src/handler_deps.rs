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
use crate::error::BotError;
use crate::metrics::BotMetrics;
use crate::model_selection::{
    InMemorySearchSessionStore, ModelCatalog, ModelChoice, ModelSelectionService,
    SqliteModelSelectionStore, StaticModelCatalog,
};
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
    pub model_selection: Arc<ModelSelectionService>,
    pub metrics: Arc<BotMetrics>,
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
    ) -> Result<Self, BotError> {
        let download_dir = settings.download_dir.clone();
        let model_catalog = build_model_catalog();
        let model_selection = Arc::new(ModelSelectionService::new(
            model_catalog,
            Arc::new(SqliteModelSelectionStore::new()),
            Arc::new(InMemorySearchSessionStore::new()),
        ));

        Ok(Self {
            sender: Arc::new(TeloxideSender::new(bot.clone())),
            agent: Arc::new(LoomAgentRunner::new(bot.clone(), Arc::clone(&settings))),
            session: Arc::new(SqliteSessionManager::new()?),
            downloader: Arc::new(TeloxideDownloader::new(bot, download_dir)),
            model_selection,
            metrics: Arc::new(BotMetrics::default()),
            settings,
            bot_username,
            run_registry,
        })
    }

    /// Test stack with explicit doubles (for example [`crate::mock`] types).
    #[allow(clippy::too_many_arguments)]
    pub fn for_test(
        sender: Arc<dyn MessageSender>,
        agent: Arc<dyn AgentRunner>,
        session: Arc<dyn SessionManager>,
        downloader: Arc<dyn FileDownloader>,
        model_selection: Arc<ModelSelectionService>,
        settings: Arc<Settings>,
        bot_username: Arc<String>,
        run_registry: Arc<ChatRunRegistry>,
    ) -> Self {
        Self {
            sender,
            agent,
            session,
            downloader,
            model_selection,
            metrics: Arc::new(BotMetrics::default()),
            settings,
            bot_username,
            run_registry,
        }
    }
}

fn build_model_catalog() -> Arc<dyn ModelCatalog> {
    let configured_model = "gpt-5.4".to_string(); // Removed environment variable support
    let full_config = config::load_full_config("loom").ok();
    let configured_provider = full_config
        .as_ref()
        .and_then(|cfg| cfg.default_provider.clone())
        .unwrap_or_else(|| "default".to_string()); // Removed environment variable support

    let mut models = vec![ModelChoice::new(configured_model.clone())];
    if let Some(full_config) = full_config {
        for provider in full_config.providers {
            if provider.name == configured_provider
                || provider.name == "openai"
                || provider.name == "gptprotoc"
            {
                if let Some(model_id) = provider.model {
                    models.push(ModelChoice::new(model_id));
                }
            }
        }
    }

    Arc::new(StaticModelCatalog::new(configured_model, models))
}
