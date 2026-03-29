use config::tracing_init::{build_env_filter, file_non_blocking_writer, resolve_log_path, LogRotate};
use telegram_bot::TelegramBotConfig;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn setup_logging(config: &TelegramBotConfig) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = build_env_filter(&config.settings.log_level, &[]);

    if let Some(log_file) = &config.settings.log_file {
        let log_path = resolve_log_path(log_file.as_path(), None);

        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match file_non_blocking_writer(&log_path, LogRotate::None, "telegram-bot") {
            Ok((writer, guard)) => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
                    .with(tracing_subscriber::fmt::layer().with_writer(writer).with_ansi(false))
                    .init();

                info!("Logging to file: {:?}", log_path);
                Some(guard)
            }
            Err(error) => {
                eprintln!("telegram-bot: {}", error);
                let fallback_filter = build_env_filter(&config.settings.log_level, &[]);
                tracing_subscriber::fmt()
                    .with_env_filter(fallback_filter)
                    .init();
                None
            }
        }
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .init();
        None
    }
}
