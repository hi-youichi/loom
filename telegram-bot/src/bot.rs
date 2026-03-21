//! Bot instance management and long polling support

use crate::config::{load_config, BotConfig, Settings, TelegramBotConfig};
use crate::handler::default_handler;
use std::sync::Arc;
use teloxide::dispatching::Dispatcher;
use teloxide::prelude::*;
use tokio::task::JoinHandle;
use tracing::{info, Instrument};

pub struct BotManager {
    pub name: String,
    pub bot: Bot,
    handle: Option<JoinHandle<()>>,
}

impl BotManager {
    pub fn new(name: String, config: &BotConfig) -> Self {
        let bot = Bot::new(&config.token);
        Self {
            name,
            bot,
            handle: None,
        }
    }

    pub async fn start(&mut self, settings: Arc<Settings>) {
        let bot = self.bot.clone();
        let name = self.name.clone();
        let span_name = name.clone();

        let me = bot.get_me().await;
        let bot_username = match me {
            Ok(m) => m.username.clone().unwrap_or_default(),
            Err(_) => String::new(),
        };
        let bot_username = Arc::new(bot_username);

        let handle = tokio::spawn(
            async move {
                info!(bot = %name, "Starting bot with long polling");

                let handler = Update::filter_message()
                    .endpoint(default_handler);

                let mut dispatcher = Dispatcher::builder(bot, handler)
                    .dependencies(dptree::deps![settings, bot_username])
                    .build();

                dispatcher.dispatch().await;
            }
            .instrument(tracing::info_span!("bot", name = %span_name)),
        );

        self.handle = Some(handle);
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

pub async fn run_with_config(config: TelegramBotConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if config.bots.is_empty() {
        return Err("No bots configured".into());
    }

    let settings = Arc::new(config.settings.clone());
    let mut managers: Vec<BotManager> = Vec::new();

    for (name, bot_config) in &config.bots {
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
