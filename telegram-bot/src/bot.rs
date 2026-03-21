//! Bot instance management and long polling support

use crate::config::{load_config, BotConfig, TelegramBotConfig};
use crate::handler::default_handler;
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

    pub fn start(&mut self) {
        let bot = self.bot.clone();
        let name = self.name.clone();
        let span_name = name.clone();

        let handle = tokio::spawn(
            async move {
                info!(bot = %name, "Starting bot with long polling");

                let handler = Update::filter_message()
                    .endpoint(default_handler);

                let mut dispatcher = Dispatcher::builder(bot, handler)
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

    let mut managers: Vec<BotManager> = Vec::new();

    for (name, bot_config) in &config.bots {
        if !bot_config.enabled {
            info!(bot = %name, "Skipping disabled bot");
            continue;
        }

        let mut manager = BotManager::new(name.clone(), bot_config);
        manager.start();
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
