//! Logging initialization with file rotation support.
//!
//! Resolution order (after `config.toml` / `.env` are applied to the process environment):
//! - `--log-level` overrides `RUST_LOG`; otherwise `RUST_LOG`, else `info`
//! - `--log-file` overrides `LOG_FILE`; when neither is set, logs are dropped (stdout stays clean)
//! - `--log-rotate`: Rotation strategy when writing to a file (none, daily, hourly, minutely)

use std::path::Path;

use config::tracing_init;
pub use config::tracing_init::LogRotate;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

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

    fn resolve_log_file(&self) -> Option<std::path::PathBuf> {
        self.file.as_ref().map(|path| {
            tracing_init::resolve_log_path(path.as_path(), self.working_folder.as_deref())
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
    let filter = tracing_init::build_env_filter(&args.level, &["hyper_util=off"]);

    let log_file = args.resolve_log_file();

    if let Some(ref path) = log_file {
        init_file_logging(path, args.rotate, filter)
    } else {
        init_sink_logging(filter)
    }
}

fn init_file_logging(path: &Path, rotate: LogRotate, filter: EnvFilter) -> LogGuard {
    let (writer, guard) = tracing_init::file_non_blocking_writer(path, rotate, "loom")
        .unwrap_or_else(|e| panic!("failed to open log file {}: {}", path.display(), e));

    let layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(writer);

    tracing_subscriber::registry().with(filter).with(layer).init();

    LogGuard { _guard: Some(guard) }
}

fn init_sink_logging(filter: EnvFilter) -> LogGuard {
    use std::io::{self, Write};

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
