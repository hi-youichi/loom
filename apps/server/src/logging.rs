//! Logging initialization for the loom HTTP server.
//!
//! Log config resolution priority:
//! 1. CLI `--log-file` argument
//! 2. `LOGS_SERVER` environment variable
//! 3. config.toml `[logging].path`
//! 4. `~/.loom/loom-server.log`
//!
//! Log level resolution priority:
//! 1. `--log-level` / `RUST_LOG` (passed in via [`LogConfig::level`])
//! 2. config.toml `[logging].level`
//! 3. `off`
//!
//! Rotation: CLI `--log-rotate` > config.toml `[logging].rotate` > `None`.
//! Format: CLI `--log-format` (`text` default, or `json`).

use std::path::{Path, PathBuf};
use std::str::FromStr;

use config::log_format::{JsonWithSpanIds, TextWithSpanIds};
use config::tracing_init;
use config::LoggingSection;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub use config::tracing_init::LogRotate;

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

/// Log configuration sourced from CLI global args.
#[derive(Debug, Clone, Default)]
pub struct LogConfig {
    /// Log level filter (e.g. "info", "debug", "loom=debug").
    pub level: String,
    /// Optional log file path (overrides env / config defaults).
    pub file: Option<PathBuf>,
    /// Rotation strategy.
    pub rotate: LogRotate,
    /// Output format (text or json).
    pub format: LogFormat,
}

/// Worker guard; keep alive for the process lifetime to flush remaining logs.
pub struct LogGuard {
    _guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// Resolves the effective server log file path.
///
/// Priority:
/// 1. CLI `file` argument
/// 2. `LOGS_SERVER` environment variable
/// 3. config.toml `[logging].path`
/// 4. `~/.loom/loom-server.log`
fn resolve_server_log_path(
    cli_file: Option<&Path>,
    logging_config: Option<&LoggingSection>,
) -> PathBuf {
    // 1. CLI argument
    if let Some(path) = cli_file {
        return tracing_init::resolve_log_path(path, None);
    }

    // 2. Environment variable
    if let Some(env_val) = std::env::var_os("LOGS_SERVER") {
        return PathBuf::from(env_val);
    }

    // 3. config.toml [logging].path
    if let Some(config) = logging_config {
        if let Some(path) = &config.path {
            return path.clone();
        }
    }

    // 4. Default: ~/.loom/loom-server.log
    config::home::loom_home().join("loom-server.log")
}

/// Resolve the effective log level, falling back to config.toml then `off`.
fn resolve_level(cli_level: &str, logging_config: Option<&LoggingSection>) -> String {
    if !cli_level.is_empty() {
        return cli_level.to_string();
    }
    if let Some(config) = logging_config {
        if let Some(level) = &config.level {
            return level.clone();
        }
    }
    "off".to_string()
}

/// Resolve rotation: CLI value takes precedence; otherwise config.toml; otherwise `None`.
fn resolve_rotate(cli_rotate: LogRotate, logging_config: Option<&LoggingSection>) -> LogRotate {
    if cli_rotate != LogRotate::None {
        return cli_rotate;
    }
    logging_config
        .map(|c| c.rotate())
        .unwrap_or(LogRotate::None)
}

/// Initialize file logging from CLI config, merged with `config.toml [logging]`.
///
/// Returns a [`LogGuard`] that must be held for the process lifetime. On failure
/// to open the log file, logs an error to stderr and returns a guard backed by a
/// null sink so the server can still start.
pub fn init_logging(config: &LogConfig) -> LogGuard {
    let logging_config = config::load_full_config("loom").ok().map(|c| c.logging);

    let log_path = resolve_server_log_path(config.file.as_deref(), logging_config.as_ref());

    // Auto-create parent directories
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let level = resolve_level(&config.level, logging_config.as_ref());
    let rotate = resolve_rotate(config.rotate, logging_config.as_ref());
    let filter = tracing_init::build_env_filter(&level, &["hyper_util=off"]);

    match tracing_init::file_non_blocking_writer(&log_path, rotate, "loom-server") {
        Ok((writer, guard)) => {
            let layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(writer);

            match config.format {
                LogFormat::Text => {
                    tracing_subscriber::registry()
                        .with(filter)
                        .with(layer.event_format(TextWithSpanIds::default()))
                        .init();
                }
                LogFormat::Json => {
                    tracing_subscriber::registry()
                        .with(filter)
                        .with(layer.event_format(JsonWithSpanIds::default()))
                        .init();
                }
            }

            tracing::info!(log_file = %log_path.display(), level = %level, "loom-server logging initialized");
            LogGuard {
                _guard: Some(guard),
            }
        }
        Err(error) => {
            eprintln!(
                "loom-server: failed to open log file {}: {error}",
                log_path.display()
            );
            init_sink_logging(filter)
        }
    }
}

/// Fallback: discard all log output (keeps the subscriber installed so
/// `tracing::` macros stay zero-cost).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_format_from_str() {
        assert_eq!(LogFormat::from_str("text").unwrap(), LogFormat::Text);
        assert_eq!(LogFormat::from_str("JSON").unwrap(), LogFormat::Json);
        assert!(LogFormat::from_str("yaml").is_err());
    }

    #[test]
    fn resolve_level_cli_wins() {
        let logging = LoggingSection {
            level: Some("debug".into()),
            ..Default::default()
        };
        assert_eq!(resolve_level("info", Some(&logging)), "info");
    }

    #[test]
    fn resolve_level_falls_back_to_config() {
        let logging = LoggingSection {
            level: Some("debug".into()),
            ..Default::default()
        };
        assert_eq!(resolve_level("", Some(&logging)), "debug");
    }

    #[test]
    fn resolve_level_defaults_to_off() {
        assert_eq!(resolve_level("", None), "off");
    }

    #[test]
    fn resolve_rotate_cli_wins() {
        let logging = LoggingSection {
            rotate: Some("hourly".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_rotate(LogRotate::Daily, Some(&logging)),
            LogRotate::Daily
        );
    }

    #[test]
    fn resolve_rotate_falls_back_to_config() {
        let logging = LoggingSection {
            rotate: Some("hourly".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_rotate(LogRotate::None, Some(&logging)),
            LogRotate::Hourly
        );
    }
}
