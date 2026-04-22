//! Delayed logging initialization for ACP.
//!
//! Log config is set at startup from CLI args, but actual file initialization
//! is delayed until the first `new_session` provides `working_folder` via ACP.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;

use config::tracing_init;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use config::log_format::{JsonWithSpanIds, TextWithSpanIds};

pub use config::tracing_init::LogRotate;

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    #[default]
    Text,
    Json,
}

impl FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            other => Err(format!(
                "unknown log format `{}`, expected `text` or `json`",
                other
            )),
        }
    }
}

/// Log configuration from CLI args.
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Log level filter (e.g., "info", "debug", "loom=debug")
    pub level: String,
    /// Optional log file path (supports {working_folder} variable)
    pub file: Option<PathBuf>,
    /// Rotation strategy
    pub rotate: LogRotate,
    /// Output format (text or json)
    pub format: LogFormat,
}

/// Initialize logging with working_folder from ACP session.
/// This should be called once when the first session is created.
/// Subsequent calls are no-ops.
pub fn init_with_working_folder(working_folder: &Path) {
    if LOG_GUARD.get().is_some() {
        return;
    }

    let config = match crate::get_log_config() {
        Some(c) => c,
        None => return,
    };

    let Some(log_file) = config
        .file
        .clone()
        .or_else(|| Some(config::home::default_acp_log_file()))
    else {
        return;
    };

    let log_path = tracing_init::resolve_log_path(log_file.as_path(), Some(working_folder));

    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let filter = tracing_init::build_env_filter(&config.level, &[]);

    let guard = match tracing_init::file_non_blocking_writer(&log_path, config.rotate, "loom-acp") {
        Ok((writer, guard)) => {
            match config.format {
                LogFormat::Text => {
                    let layer = tracing_subscriber::fmt::layer()
                        .with_ansi(false)
                        .event_format(TextWithSpanIds::default())
                        .with_writer(writer);
                    tracing_subscriber::registry()
                        .with(filter)
                        .with(layer)
                        .init();
                }
                LogFormat::Json => {
                    let layer = tracing_subscriber::fmt::layer()
                        .with_ansi(false)
                        .event_format(JsonWithSpanIds::default())
                        .with_writer(writer);
                    tracing_subscriber::registry()
                        .with(filter)
                        .with(layer)
                        .init();
                }
            }
            Some(guard)
        }
        Err(e) => {
            eprintln!("loom-acp: {}", e);
            None
        }
    };

    if let Some(g) = guard {
        let _ = LOG_GUARD.set(g);
    }
}
