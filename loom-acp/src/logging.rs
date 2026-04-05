//! Delayed logging initialization for ACP.
//!
//! Log config is set at startup from CLI args, but actual file initialization
//! is delayed until the first `new_session` provides `working_folder` via ACP.

use std::path::PathBuf;
use std::sync::OnceLock;

use config::tracing_init;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub use config::tracing_init::LogRotate;

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Log configuration from CLI args.
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Log level filter (e.g., "info", "debug", "loom=debug")
    pub level: String,
    /// Optional log file path (supports {working_folder} variable)
    pub file: Option<PathBuf>,
    /// Rotation strategy
    pub rotate: LogRotate,
}

/// Initialize logging with working_folder from ACP session.
/// This should be called once when the first session is created.
/// Subsequent calls are no-ops.
pub fn init_with_working_folder(working_folder: &PathBuf) {
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

    let log_path =
        tracing_init::resolve_log_path(log_file.as_path(), Some(working_folder.as_path()));

    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let filter = tracing_init::build_env_filter(&config.level, &[]);

    let guard = match tracing_init::file_non_blocking_writer(&log_path, config.rotate, "loom-acp") {
        Ok((writer, guard)) => {
            let layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_writer(writer);
            tracing_subscriber::registry()
                .with(filter)
                .with(layer)
                .init();
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
