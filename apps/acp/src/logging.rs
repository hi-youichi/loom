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
use config::LoggingSection;

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
    /// Log level filter (e.g., "info", "debug", "anureo=debug")
    pub level: String,
    /// Optional log file path (supports {working_folder} variable)
    pub file: Option<PathBuf>,
    /// Rotation strategy
    pub rotate: LogRotate,
    /// Output format (text or json)
    pub format: LogFormat,
}

/// Resolves the effective ACP log file path.
///
/// Priority:
/// 1. CLI `file` argument
/// 2. `LOGS_ACP` environment variable
/// 3. `config.toml` [logging].path
/// 4. `~/.anureo/anureo.log`
pub fn resolve_acp_log_path(
    cli_file: Option<&Path>,
    logging_config: Option<&LoggingSection>,
) -> Option<PathBuf> {
    // 1. CLI argument
    if let Some(path) = cli_file {
        return Some(path.to_path_buf());
    }

    // 2. Environment variable
    if let Some(env_val) = std::env::var_os("LOGS_ACP") {
        return Some(PathBuf::from(env_val));
    }

    // 3. config.toml [logging].path
    if let Some(config) = logging_config {
        if let Some(path) = &config.path {
            return Some(path.clone());
        }
    }

    // 4. Default: ~/.anureo/anureo.log
    Some(config::home::default_log_file())
}

/// Initialize logging at application startup.
/// - If no working_folder is provided, relative paths are resolved relative to the current process working directory.
/// - If working_folder is provided, relative paths are resolved relative to that folder.
///
/// This should be called once at startup. Subsequent calls are no-ops.
pub fn init_logging(working_folder: Option<&Path>) {
    if LOG_GUARD.get().is_some() {
        return;
    }

    let config = match crate::get_log_config() {
        Some(c) => c,
        None => return,
    };

    // Load logging config from config.toml
    let logging_config = config::load_full_config("anureo").ok().map(|c| c.logging);
    let log_file = resolve_acp_log_path(config.file.as_deref(), logging_config.as_ref());

    let Some(log_file) = log_file else {
        return;
    };

    let log_path = tracing_init::resolve_log_path(log_file.as_path(), working_folder);

    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Resolve rotate from config if not set by CLI
    let rotate = if config.rotate != LogRotate::None {
        config.rotate
    } else if let Some(ref cfg) = logging_config {
        cfg.rotate()
    } else {
        LogRotate::None
    };

    let filter = tracing_init::build_env_filter(&config.level, &[]);

    let guard = match tracing_init::file_non_blocking_writer(&log_path, rotate, "anureo-acp") {
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
            eprintln!("anureo-acp: {}", e);
            None
        }
    };

    if let Some(g) = guard {
        let _ = LOG_GUARD.set(g);
    }
}
