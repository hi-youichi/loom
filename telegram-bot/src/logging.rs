use std::path::PathBuf;

use telegram_bot::TelegramBotConfig;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn setup_logging(config: &TelegramBotConfig) -> Option<tracing_appender::non_blocking::WorkerGuard> {
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
