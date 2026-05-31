//! Process startup: config report and logging.

use crate::args::Args;
use crate::logging;

use std::ffi::OsString;

/// Saves shell environment variables that config.toml might override.
/// Call this BEFORE `print_config_report` / `load_and_apply`.
pub(crate) fn preserve_shell_env() -> ShellEnv {
    ShellEnv {
        log_file: std::env::var_os("LOG_FILE"),
    }
}

/// Snapshot of shell environment variables (before config.toml overrides them).
pub(crate) struct ShellEnv {
    pub log_file: Option<OsString>,
}

pub(crate) fn print_config_report() {
    if std::env::var("LOOM_TEST_MODE").is_ok() {
        return;
    }
    if let Ok(report) = config::load_and_apply_with_report("loom", None::<&std::path::Path>) {
        if let Some(p) = &report.dotenv_path {
            let full = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            eprintln!("config: .env path={}", full.display());
        }
        if let Some(p) = &report.xdg_path {
            let full = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            eprintln!("config: config.toml path={}", full.display());
        }
        if let Some(ref provider) = report.active_provider {
            eprintln!("config: provider={}", provider);
        }
        if let Some(keys) = report.keys_summary() {
            eprintln!("{}", keys);
        }
    }
}

/// Initializes logging with config.toml support.
///
/// Resolution order:
/// 1. CLI args (`--log-file`, `--log-level`, etc.)
/// 2. Shell environment variables (`LOG_FILE`, `RUST_LOG`) — captured before config.toml loaded
/// 3. config.toml `[logging.cli]` section
/// 4. Defaults (`~/.loom/logs/cli/loom-cli.log`, no rotation)
pub(crate) fn init_logging(args: &Args, shell_env: ShellEnv) -> logging::LogGuard {
    let log_level = args
        .log_level
        .clone()
        .or_else(|| {
            std::env::var("RUST_LOG")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or_else(|| "info".to_string());

    // Load logging config from config.toml
    let logging_config = config::load_full_config("loom")
        .ok()
        .map(|c| c.logging);

    // Determine the effective log file path, considering:
    // - If CLI --log-file was explicitly set, use it
    // - If shell LOG_FILE was set before config.toml, use it
    // - Otherwise use config.toml [logging.cli].path or default
    eprintln!("DEBUG: args.log_file.is_some() = {}", args.log_file.is_some());
    eprintln!("DEBUG: shell_env.log_file = {:?}", shell_env.log_file);
    let log_file_path = if args.log_file.is_some() {
        // CLI --log-file was explicitly set
        args.log_file.clone()
    } else {
        logging::resolve_cli_log_path(
            None,
            args.working_folder.as_deref(),
            logging_config.as_ref(),
            shell_env.log_file.as_ref(),
        )
    };

    eprintln!("DEBUG: log_file_path = {:?}", log_file_path);

    let log_args = logging::LogArgs::new(
        log_level,
        log_file_path,
        &args.log_rotate,
        &args.log_format,
        args.working_folder.clone(),
    );

    logging::init_with_config(
        &log_args,
        logging_config.as_ref(),
        shell_env.log_file.as_ref(),
    )
}
