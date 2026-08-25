//! Process startup: config report and logging.

use crate::args::Args;
use crate::logging;

use std::ffi::OsString;

/// Map `-v`/`-vv`/`-vvv` to a tracing EnvFilter level.
///
/// | verbose | level  |
/// |---------|--------|
/// | 0       | `off`  |
/// | 1       | `error`|
/// | 2       | `warn` |
/// | 3       | `info` |
/// | 4+      | `debug`|
pub(crate) fn verbose_to_level(verbose: u8) -> &'static str {
    match verbose {
        0 => "off",
        1 => "error",
        2 => "warn",
        3 => "info",
        _ => "debug",
    }
}

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

pub(crate) fn print_config_report(verbose: u8) {
    if verbose < 3 || std::env::var("ANUREO_TEST_MODE").is_ok() {
        return;
    }
    if let Ok(report) = config::load_and_apply_with_report("anureo", None::<&std::path::Path>) {
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
/// 3. config.toml `[logging]` section
/// 4. `~/.anureo/anureo.log` (default), level from `-v`/`-vv`/`-vvv` (default: `off`)
pub(crate) fn init_logging(args: &Args, shell_env: ShellEnv) -> logging::LogGuard {
    // Load logging config from config.toml
    let logging_config = config::load_full_config("anureo").ok().map(|c| c.logging);

    // Log level: --log-level > RUST_LOG (shell) > [logging].level > verbosity-based > off
    let log_level = args
        .log_level
        .clone()
        .or_else(|| {
            std::env::var("RUST_LOG")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .or_else(|| {
            logging_config
                .as_ref()
                .and_then(|c| c.level.clone())
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or_else(|| verbose_to_level(args.verbose).to_string());

    // Determine the effective log file path
    let log_file_path = if args.log_file.is_some() {
        args.log_file.clone()
    } else {
        logging::resolve_cli_log_path(
            None,
            args.working_folder.as_deref(),
            logging_config.as_ref(),
            shell_env.log_file.as_ref(),
        )
    };

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
