//! Logging initialization with file rotation support.
//!
//! Resolution order (after `config.toml` / `.env` are applied to the process environment):
//! - `--log-level` overrides `RUST_LOG`; otherwise `RUST_LOG`, else `info`
//! - `--log-file` overrides `LOG_FILE`; when neither is set, logs are dropped (stdout stays clean)
//! - `--log-rotate`: Rotation strategy when writing to a file (none, daily, hourly, minutely)

use std::path::Path;

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Log rotation strategy.
#[derive(Debug, Clone, Copy, Default)]
pub enum LogRotate {
    /// No rotation, append to single file
    None,
    /// Rotate daily (default)
    #[default]
    Daily,
    /// Rotate hourly
    Hourly,
    /// Rotate every minute (for testing)
    Minutely,
}

impl LogRotate {
    /// Parse from string (CLI argument).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "none" => Some(Self::None),
            "daily" => Some(Self::Daily),
            "hourly" => Some(Self::Hourly),
            "minutely" => Some(Self::Minutely),
            _ => None,
        }
    }
}

/// Log configuration from CLI args.
#[derive(Debug, Clone)]
pub struct LogArgs {
    /// Log level filter (e.g., "info", "debug", "loom=debug")
    pub level: String,
    /// Optional log file path (supports {working_folder} variable)
    pub file: Option<std::path::PathBuf>,
    /// Rotation strategy
    pub rotate: LogRotate,
    /// Working folder for variable substitution in log file path
    pub working_folder: Option<std::path::PathBuf>,
}

impl LogArgs {
    /// Create log args from CLI arguments.
    pub fn new(
        level: String,
        file: Option<std::path::PathBuf>,
        rotate: &str,
        working_folder: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            level,
            file,
            rotate: LogRotate::from_str(rotate).unwrap_or_default(),
            working_folder,
        }
    }

    /// Resolve log file path, substituting {working_folder} variable if present.
    /// For relative paths without {working_folder}, resolves against working_folder if provided.
    fn resolve_log_file(&self) -> Option<std::path::PathBuf> {
        self.file.as_ref().map(|path| {
            let path_str = path.to_string_lossy();
            if path_str.contains("{working_folder}") {
                if let Some(wf) = &self.working_folder {
                    std::path::PathBuf::from(
                        path_str.replace("{working_folder}", &wf.to_string_lossy()),
                    )
                } else {
                    // If working_folder not provided but variable is used, keep as-is
                    // (will likely fail when trying to create the file)
                    path.clone()
                }
            } else if !path.is_absolute() {
                // For relative paths, resolve against working_folder if provided
                if let Some(wf) = &self.working_folder {
                    wf.join(path)
                } else {
                    path.clone()
                }
            } else {
                path.clone()
            }
        })
    }
}

/// Worker guard that keeps the log file writer alive.
/// Drop this to flush remaining logs.
pub struct LogGuard {
    _guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// Initializes tracing with optional file logging and rotation.
///
/// - With a resolved log file path (`--log-file` or `LOG_FILE`): logs go to file (with rotation) only
/// - Without: logs are dropped (sink)
///
/// Returns `LogGuard` that must be kept alive for file logging to work.
/// Panics if file logging fails to initialize.
pub fn init(args: &LogArgs) -> LogGuard {
    let filter = EnvFilter::try_new(&args.level)
        .or_else(|_| EnvFilter::try_new("info"))
        .expect("valid log level");

    // Add hyper_util=off by default to reduce noise
    let filter = filter
        .add_directive("hyper_util=off".parse().expect("valid directive"));

    // Resolve log file path (handles {working_folder} variable and relative paths)
    let log_file = args.resolve_log_file();

    if let Some(ref path) = log_file {
        init_file_logging(path, args.rotate, filter)
    } else {
        // No log file: drop all logs (sink)
        init_sink_logging(filter)
    }
}

fn init_file_logging(path: &Path, rotate: LogRotate, filter: EnvFilter) -> LogGuard {
    let (writer, guard) = match rotate {
        LogRotate::None => {
            // No rotation: use simple file append
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap_or_else(|e| panic!("failed to open log file {}: {}", path.display(), e));
            tracing_appender::non_blocking(file)
        }
        LogRotate::Daily => {
            let parent = path.parent().unwrap_or(Path::new("."));
            let prefix = path.file_stem().and_then(|s| s.to_str()).unwrap_or("loom");
            let suffix = path.extension().and_then(|s| s.to_str()).unwrap_or("log");
            let appender = tracing_appender::rolling::RollingFileAppender::builder()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix(prefix)
                .filename_suffix(suffix)
                .build(parent)
                .unwrap_or_else(|e| panic!("failed to create log file: {}", e));
            tracing_appender::non_blocking(appender)
        }
        LogRotate::Hourly => {
            let parent = path.parent().unwrap_or(Path::new("."));
            let prefix = path.file_stem().and_then(|s| s.to_str()).unwrap_or("loom");
            let suffix = path.extension().and_then(|s| s.to_str()).unwrap_or("log");
            let appender = tracing_appender::rolling::RollingFileAppender::builder()
                .rotation(tracing_appender::rolling::Rotation::HOURLY)
                .filename_prefix(prefix)
                .filename_suffix(suffix)
                .build(parent)
                .unwrap_or_else(|e| panic!("failed to create log file: {}", e));
            tracing_appender::non_blocking(appender)
        }
        LogRotate::Minutely => {
            let parent = path.parent().unwrap_or(Path::new("."));
            let prefix = path.file_stem().and_then(|s| s.to_str()).unwrap_or("loom");
            let suffix = path.extension().and_then(|s| s.to_str()).unwrap_or("log");
            let appender = tracing_appender::rolling::RollingFileAppender::builder()
                .rotation(tracing_appender::rolling::Rotation::MINUTELY)
                .filename_prefix(prefix)
                .filename_suffix(suffix)
                .build(parent)
                .unwrap_or_else(|e| panic!("failed to create log file: {}", e));
            tracing_appender::non_blocking(appender)
        }
    };

    let layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(writer);

    tracing_subscriber::registry().with(filter).with(layer).init();

    LogGuard { _guard: Some(guard) }
}

fn init_sink_logging(filter: EnvFilter) -> LogGuard {
    use std::io::{self, Write};

    // Sink that discards all writes
    struct Sink;

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let (writer, guard) = tracing_appender::non_blocking(Sink);

    let layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(writer);

    tracing_subscriber::registry().with(filter).with(layer).init();

    LogGuard { _guard: Some(guard) }
}
