//! Startup validation for [`telegram_bot::run_with_config`] (maps: E2E-TG-028, empty config).

use std::collections::HashMap;

use telegram_bot::{run_with_config, BotConfig, Settings, TelegramBotConfig};

fn disabled_bot(token: &str) -> BotConfig {
    BotConfig {
        token: token.to_string(),
        enabled: false,
        description: None,
        handler: None,
    }
}

#[tokio::test]
async fn e2e_tg_028_no_enabled_bots_returns_error() {
    let mut bots = HashMap::new();
    bots.insert("bot_a".to_string(), disabled_bot("1:AAA"));
    bots.insert("bot_b".to_string(), disabled_bot("2:BBB"));

    let config = TelegramBotConfig {
        settings: Settings::default(),
        bots,
        agent: None,
    };

    let err = run_with_config(config)
        .await
        .expect_err("expected startup error");
    let msg = format!("{err}");
    assert!(
        msg.contains("No enabled bots"),
        "unexpected error message: {msg}"
    );
}

#[tokio::test]
async fn run_with_config_empty_bot_map_returns_error() {
    let config = TelegramBotConfig {
        settings: Settings::default(),
        bots: HashMap::new(),
        agent: None,
    };

    let err = run_with_config(config).await.expect_err("expected error");
    let msg = format!("{err}");
    assert!(
        msg.contains("No bots configured"),
        "unexpected error message: {msg}"
    );
}
