use telegram_bot::{load_config, run_with_config};
use tracing::{error, info};

mod logging;

const TELEGRAM_BOT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // NOTE: eprintln! used here because tracing is initialized after config loads.
    match config::load_and_apply_with_report("loom", None::<&std::path::Path>) {
        Ok(report) => {
            if let Some(p) = &report.dotenv_path {
                eprintln!("config: .env path={}", p.display());
            }
            if let Some(p) = &report.xdg_path {
                eprintln!("config: config.toml path={}", p.display());
            }
            if let Some(ref provider) = report.active_provider {
                eprintln!("config: provider={}", provider);
            }
        }
        Err(e) => {
            eprintln!(
                "config: loom global config not found (non-fatal, continuing): {}",
                e
            );
        }
    }

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

    let _guard = logging::setup_logging(&config);

    info!("Starting Telegram Bot Manager v{}...", TELEGRAM_BOT_VERSION);
    info!("telegram-bot version: {}", TELEGRAM_BOT_VERSION);
    info!("Log level: {}", config.settings.log_level);

    if let Err(e) = run_with_config(config).await {
        error!("Bot manager error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
