use std::path::PathBuf;
use telegram_bot::{load_config, run_with_config, TelegramBotConfig};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

fn setup_logging(config: &TelegramBotConfig) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.settings.log_level));

    if let Some(log_file) = &config.settings.log_file {
        let log_path = if log_file.is_absolute() {
            log_file.clone()
        } else {
            PathBuf::from(".").join(log_file)
        };

        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let file_appender = tracing_appender::rolling::never(
            log_path.parent().unwrap_or(PathBuf::from(".").as_path()),
            log_path.file_name().unwrap_or(std::ffi::OsStr::new("telegram-bot.log")),
        );
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
            .with(tracing_subscriber::fmt::layer().with_writer(non_blocking).with_ansi(false))
            .init();

        info!("Logging to file: {:?}", log_path);
        Some(guard)
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .init();
        None
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = match load_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {}", e);
            eprintln!("\nPlease ensure you have a configuration file at:");
            eprintln!("  - ~/.loom/telegram-bot.toml (recommended)");
            eprintln!("  - ./telegram-bot.toml (current directory)");
            eprintln!("\nYou can copy the example config:");
            eprintln!("  cp telegram-bot/telegram-bot.example.toml ~/.loom/telegram-bot.toml");
            std::process::exit(1);
        }
    };

    let _guard = setup_logging(&config);

    info!("Starting Telegram Bot Manager...");
    info!("Log level: {}", config.settings.log_level);

    if let Err(e) = run_with_config(config).await {
        error!("Bot manager error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
