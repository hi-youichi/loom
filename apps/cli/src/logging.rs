//! Logging initialization with file rotation support.
//!
//! Resolution order (after `config.toml` / `.env` are applied to the process environment):
//! - `--log-level` overrides `RUST_LOG`; otherwise `RUST_LOG`, else `[logging].level` (config.toml), else verbosity (`-v`/`-vv`/`-vvv`), else `off`
//! - `--log-file` overrides `LOG_FILE`; default: `~/.loom/loom.log`
//! - `--log-format`: `text` (default) or `json`
//! - `--log-rotate`: Rotation strategy when writing to a file (none, daily, hourly, minutely)
//!
//! Default log location: `~/.loom/loom.log`

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use config::tracing_init;
pub use config::tracing_init::LogRotate;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use config::log_format::{JsonWithSpanIds, TextWithSpanIds};
use config::LoggingSection;

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

/// Log configuration from CLI args and config.toml.
#[derive(Debug, Clone)]
pub struct LogArgs {
    /// Log level filter (e.g., "info", "debug", "loom=debug")
    pub level: String,
    /// Optional log file path (supports {working_folder} variable)
    pub file: Option<std::path::PathBuf>,
    /// Rotation strategy
    pub rotate: LogRotate,
    /// Output format (text or json)
    pub format: LogFormat,
    /// Working folder for variable substitution in log file path
    pub working_folder: Option<std::path::PathBuf>,
}

impl LogArgs {
    /// Create log args from CLI arguments.
    pub fn new(
        level: String,
        file: Option<std::path::PathBuf>,
        rotate: &str,
        format: &str,
        working_folder: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            level,
            file,
            rotate: LogRotate::from_str(rotate).unwrap_or_default(),
            format: LogFormat::from_str(format).unwrap_or_default(),
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

/// Resolves the effective log file path from CLI args, environment, or defaults.
///
/// Priority:
/// 1. `--log-file` CLI argument
/// 2. `LOG_FILE` environment variable (only from shell, not from config.toml [env])
/// 3. `config.toml` [logging].path
/// 4. `~/.loom/loom.log`
pub fn resolve_cli_log_path(
    cli_file: Option<&Path>,
    working_folder: Option<&Path>,
    logging_config: Option<&LoggingSection>,
    log_file_from_env: Option<&OsString>,
) -> Option<PathBuf> {
    // 1. CLI argument
    if let Some(path) = cli_file {
        return Some(tracing_init::resolve_log_path(path, working_folder));
    }

    // 2. Environment variable (only from shell env, not from config.toml [env])
    // We check if LOG_FILE was set in the shell before config.toml was loaded.
    // If the logging_config has cli.path set, that takes precedence over [env] LOG_FILE.
    if let Some(path) = log_file_from_env {
        if logging_config
            .as_ref()
            .and_then(|c| c.path.as_ref())
            .is_none()
        {
            return Some(PathBuf::from(path));
        }
    }

    // 3. config.toml [logging].path
    if let Some(config) = logging_config {
        if let Some(path) = &config.path {
            return Some(path.clone());
        }
    }

    // 4. Default: ~/.loom/loom.log
    Some(config::home::default_log_file())
}

/// Initializes tracing with logging config from config.toml.
///
/// Takes the `logging` section from FullConfig and uses it for default values.
/// Priority: CLI args > shell env vars > config.toml > defaults.
pub fn init_with_config(
    args: &LogArgs,
    logging_config: Option<&LoggingSection>,
    log_file_from_shell: Option<&OsString>,
) -> LogGuard {
    let filter = tracing_init::build_env_filter(&args.level, &["hyper_util=off"]);

    let log_file = if let Some(path) = args.resolve_log_file() {
        Some(path)
    } else {
        resolve_cli_log_path(
            None,
            args.working_folder.as_deref(),
            logging_config,
            log_file_from_shell,
        )
    };

    // Resolve rotate from config if not set by CLI
    let rotate = if args.rotate != LogRotate::None {
        args.rotate
    } else if let Some(config) = logging_config {
        config.rotate()
    } else {
        LogRotate::None
    };

    if let Some(ref path) = log_file {
        init_file_logging(path, rotate, args.format, filter)
    } else {
        init_sink_logging(filter)
    }
}

fn init_file_logging(
    path: &Path,
    rotate: LogRotate,
    format: LogFormat,
    filter: EnvFilter,
) -> LogGuard {
    // Auto-create parent directories
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let (writer, guard) = tracing_init::file_non_blocking_writer(path, rotate, "loom-cli")
        .unwrap_or_else(|e| panic!("failed to open log file {}: {}", path.display(), e));

    match format {
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

    LogGuard {
        _guard: Some(guard),
    }
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

    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .event_format(TextWithSpanIds::default())
        .with_writer(writer);

    tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .init();

    LogGuard {
        _guard: Some(guard),
    }
}
