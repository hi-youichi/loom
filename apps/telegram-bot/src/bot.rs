//! Bot instance management and long polling support

use crate::config::{load_config, BotConfig, Settings, TelegramBotConfig};
use crate::error::BotError;
use crate::handler_deps::ChatRunRegistry;
use crate::router::default_handler;
use std::sync::Arc;
use std::time::Duration;
use teloxide::dispatching::Dispatcher;
use teloxide::prelude::*;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, Instrument};

pub struct BotManager {
    pub name: String,
    pub bot: Bot,
    handle: Option<JoinHandle<()>>,
    cancellation_token: CancellationToken,
    _max_restarts: u32,
    _restart_delay: Duration,
}

impl BotManager {
    pub fn new(name: String, config: &BotConfig) -> Self {
        let bot = Bot::new(&config.token);
        Self {
            name,
            bot,
            handle: None,
            cancellation_token: CancellationToken::new(),
            _max_restarts: 3,
            _restart_delay: Duration::from_secs(5),
        }
    }

    pub async fn start(&mut self, settings: Arc<Settings>) {
        let bot = self.bot.clone();
        let name = self.name.clone();
        let span_name = name.clone();
        let cancellation_token = self.cancellation_token.clone();

        // Initialize Telegram API for anureo tools
        crate::telegram_tools::init_telegram_api(bot.clone());

        // Hermes `gateway/run.py:62-72` analogue: bound the startup
        // `get_me()` call with a 30s timeout. Without this, a hung
        // Telegram API (rate-limit, network partition, regional
        // outage) wedges the bot process during boot and never
        // produces a diagnostic. A missing `username` is non-fatal —
        // it only disables @-mention gating, the rest of the bot
        // continues to function.
        const GET_ME_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
        let me = tokio::time::timeout(GET_ME_TIMEOUT, bot.get_me())
            .await
            .map_err(|_| {
                BotError::Config(format!(
                    "telegram get_me() timed out after {}s — continuing with empty username",
                    GET_ME_TIMEOUT.as_secs()
                ))
            })
            .and_then(|res| res.map_err(|e| BotError::Network(Box::new(e))));
        let bot_username = match me {
            Ok(m) => m.username.clone().unwrap_or_default(),
            Err(_) => String::new(),
        };
        let bot_username = Arc::new(bot_username);
        info!(
            bot = %name,
            bot_username = %bot_username,
            only_respond_when_mentioned = settings.only_respond_when_mentioned,
            "Resolved bot runtime configuration"
        );

        let run_registry = Arc::new(ChatRunRegistry::new());

        let handle = tokio::spawn(
            async move {
                info!(bot = %name, "Starting bot with long polling");

                let handler = Update::filter_message().endpoint(default_handler);

                let mut dispatcher = Dispatcher::builder(bot, handler)
                    .dependencies(dptree::deps![settings, bot_username, run_registry])
                    .build();

                tokio::select! {
                    _ = cancellation_token.cancelled() => {
                        info!(bot = %name, "Bot shutting down gracefully");
                    }
                    _ = dispatcher.dispatch() => {}
                }
            }
            .instrument(tracing::info_span!("bot", name = %span_name)),
        );

        self.handle = Some(handle);
    }

    pub async fn shutdown(mut self) {
        self.cancellation_token.cancel();
        if let Some(handle) = self.handle.take() {
            match tokio::time::timeout(Duration::from_secs(10), handle).await {
                Ok(_) => info!(bot = %self.name, "Bot shutdown complete"),
                Err(_) => error!(bot = %self.name, "Bot shutdown timeout"),
            }
        }
    }

    pub async fn join(mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }
}

pub async fn run_bots() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = load_config()?;
    run_with_config(config).await
}

pub async fn run_with_config(
    config: TelegramBotConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if config.bots.is_empty() {
        return Err("No bots configured".into());
    }

    let settings = Arc::new(config.settings.clone());
    let enabled_bot_count = config.bots.values().filter(|bot| bot.enabled).count();
    info!(
        configured_bot_count = config.bots.len(),
        enabled_bot_count,
        only_respond_when_mentioned = settings.only_respond_when_mentioned,
        log_level = %settings.log_level,
        log_file = ?settings.log_file,
        "Starting Telegram bot manager with configuration"
    );
    let mut managers: Vec<BotManager> = Vec::new();

    for (name, bot_config) in &config.bots {
        info!(
            bot = %name,
            enabled = bot_config.enabled,
            description = ?bot_config.description,
            "Loaded bot configuration"
        );

        if !bot_config.enabled {
            info!(bot = %name, "Skipping disabled bot");
            continue;
        }

        let mut manager = BotManager::new(name.clone(), bot_config);
        manager.start(Arc::clone(&settings)).await;
        managers.push(manager);

        info!(bot = %name, "Bot initialized");
    }

    if managers.is_empty() {
        return Err("No enabled bots".into());
    }

    info!("All bots started, waiting for messages...");

    for manager in managers {
        manager.join().await;
    }

    Ok(())
}
