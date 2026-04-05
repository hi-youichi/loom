//! Tracing file logging helpers shared by CLI and `loom-acp`.
//!
//! Requires crate feature `tracing-init`. Callers own subscriber `init()` and guard lifetime.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use thiserror::Error;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::EnvFilter;

/// Log rotation strategy for file appenders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogRotate {
    /// Append to a single file.
    None,
    /// Rotate daily (default for ambiguous CLI input).
    #[default]
    Daily,
    /// Rotate hourly.
    Hourly,
    /// Rotate every minute (testing).
    Minutely,
}

impl FromStr for LogRotate {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "none" => Ok(Self::None),
            "daily" => Ok(Self::Daily),
            "hourly" => Ok(Self::Hourly),
            "minutely" => Ok(Self::Minutely),
            _ => Err(()),
        }
    }
}

impl LogRotate {
    /// Parse from a CLI string; unknown values return `None`.
    pub fn parse(s: &str) -> Option<Self> {
        Self::from_str(s).ok()
    }

    /// Parse like ACP: unknown strings map to [`LogRotate::Daily`].
    pub fn from_str_or_daily(s: &str) -> Self {
        Self::parse(s).unwrap_or(Self::Daily)
    }
}

/// Resolve a log path template: `{working_folder}` substitution, then relative paths against
/// `working_folder` when provided (otherwise relative paths stay as-is).
pub fn resolve_log_path(path: &Path, working_folder: Option<&Path>) -> PathBuf {
    let path_str = path.to_string_lossy();
    if path_str.contains("{working_folder}") {
        if let Some(wf) = working_folder {
            PathBuf::from(path_str.replace("{working_folder}", &wf.to_string_lossy()))
        } else {
            path.to_path_buf()
        }
    } else if !path.is_absolute() {
        if let Some(wf) = working_folder {
            wf.join(path)
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    }
}

/// Build `EnvFilter` from a level string, with optional extra directives (e.g. `hyper_util=off`).
pub fn build_env_filter(level: &str, extra_directives: &[&str]) -> EnvFilter {
    let mut filter = EnvFilter::try_new(level)
        .or_else(|_| EnvFilter::try_new("info"))
        .expect("valid log level");
    for d in extra_directives {
        if let Ok(dir) = d.parse() {
            filter = filter.add_directive(dir);
        }
    }
    filter
}

/// Error opening or building the log writer.
#[derive(Debug, Error)]
pub enum FileWriterError {
    #[error("failed to open log file {path}: {source}")]
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to create rolling log appender: {0}")]
    Rolling(String),
}

/// Non-blocking writer and guard; keep the guard alive for the process lifetime.
pub fn file_non_blocking_writer(
    path: &Path,
    rotate: LogRotate,
    rolling_default_stem: &str,
) -> Result<(NonBlocking, WorkerGuard), FileWriterError> {
    match rotate {
        LogRotate::None => {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| FileWriterError::Open {
                    path: path.to_path_buf(),
                    source: e,
                })?;
            Ok(tracing_appender::non_blocking(file))
        }
        LogRotate::Daily | LogRotate::Hourly | LogRotate::Minutely => {
            let parent = path.parent().unwrap_or(Path::new("."));
            let prefix = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(rolling_default_stem);
            let suffix = path.extension().and_then(|s| s.to_str()).unwrap_or("log");
            let rotation = match rotate {
                LogRotate::Daily => tracing_appender::rolling::Rotation::DAILY,
                LogRotate::Hourly => tracing_appender::rolling::Rotation::HOURLY,
                LogRotate::Minutely => tracing_appender::rolling::Rotation::MINUTELY,
                LogRotate::None => unreachable!(),
            };
            let appender = tracing_appender::rolling::RollingFileAppender::builder()
                .rotation(rotation)
                .filename_prefix(prefix)
                .filename_suffix(suffix)
                .build(parent)
                .map_err(|e| FileWriterError::Rolling(e.to_string()))?;
            Ok(tracing_appender::non_blocking(appender))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_relative_without_working_folder_unchanged() {
        let p = Path::new("logs/x.log");
        assert_eq!(resolve_log_path(p, None), PathBuf::from("logs/x.log"));
    }

    #[test]
    fn resolve_relative_with_working_folder_joined() {
        let wf = Path::new("/proj");
        let p = Path::new("logs/x.log");
        assert_eq!(
            resolve_log_path(p, Some(wf)),
            PathBuf::from("/proj/logs/x.log")
        );
    }

    #[test]
    fn resolve_absolute_ignores_working_folder() {
        let wf = Path::new("/proj");
        let p = Path::new("/var/log/x.log");
        assert_eq!(
            resolve_log_path(p, Some(wf)),
            PathBuf::from("/var/log/x.log")
        );
    }

    #[test]
    fn resolve_working_folder_placeholder() {
        let wf = Path::new("/workspace");
        let p = Path::new("{working_folder}/out.log");
        assert_eq!(
            resolve_log_path(p, Some(wf)),
            PathBuf::from("/workspace/out.log")
        );
    }

    #[test]
    fn log_rotate_from_str_or_daily_unknown() {
        assert_eq!(LogRotate::from_str_or_daily("bogus"), LogRotate::Daily);
        assert_eq!(LogRotate::from_str_or_daily("none"), LogRotate::None);
    }
}
