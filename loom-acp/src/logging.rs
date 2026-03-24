//! Delayed logging initialization for ACP.
//!
//! Log config is set at startup from CLI args, but actual file initialization
//! is delayed until the first `new_session` provides `working_folder` via ACP.

use std::path::PathBuf;
use std::sync::OnceLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

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

/// Log rotation strategy.
#[derive(Debug, Clone, Copy, Default)]
pub enum LogRotate {
    #[default]
    Daily,
    Hourly,
    Minutely,
    None,
}

impl LogRotate {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "none" => Self::None,
            "hourly" => Self::Hourly,
            "minutely" => Self::Minutely,
            _ => Self::Daily,
        }
    }
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
        .or_else(|| Some(config::home::loom_home().join("acp").join("loom-acp.log")))
    else {
        return;
    };

    let log_path = resolve_working_folder(&log_file, working_folder);

    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let filter = EnvFilter::try_new(&config.level)
        .or_else(|_| EnvFilter::try_new("info"))
        .expect("valid log level");

    let guard = match config.rotate {
        LogRotate::None => init_no_rotation(&log_path, filter),
        LogRotate::Daily | LogRotate::Hourly | LogRotate::Minutely => {
            init_with_rotation(&log_path, &config.rotate, filter)
        }
    };

    if let Some(g) = guard {
        let _ = LOG_GUARD.set(g);
    }
}

fn init_no_rotation(
    log_path: &PathBuf,
    filter: EnvFilter,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("loom-acp: failed to create log file: {}", e);
            return None;
        }
    };

    let (writer, guard) = tracing_appender::non_blocking(file);
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

fn init_with_rotation(
    log_path: &PathBuf,
    rotate: &LogRotate,
    filter: EnvFilter,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let parent = log_path.parent().unwrap_or(std::path::Path::new("."));
    let prefix = log_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("loom-acp");
    let suffix = log_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("log");

    let rotation = match rotate {
        LogRotate::Daily => tracing_appender::rolling::Rotation::DAILY,
        LogRotate::Hourly => tracing_appender::rolling::Rotation::HOURLY,
        LogRotate::Minutely => tracing_appender::rolling::Rotation::MINUTELY,
        LogRotate::None => unreachable!(),
    };

    let appender = match tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(rotation)
        .filename_prefix(prefix)
        .filename_suffix(suffix)
        .build(parent)
    {
        Ok(a) => a,
        Err(e) => {
            eprintln!("loom-acp: failed to create log file: {}", e);
            return None;
        }
    };

    let (writer, guard) = tracing_appender::non_blocking(appender);
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

fn resolve_working_folder(path: &PathBuf, working_folder: &PathBuf) -> PathBuf {
    let path_str = path.to_string_lossy();
    if path_str.contains("{working_folder}") {
        PathBuf::from(path_str.replace("{working_folder}", &working_folder.to_string_lossy()))
    } else if !path.is_absolute() {
        working_folder.join(path)
    } else {
        path.clone()
    }
}
