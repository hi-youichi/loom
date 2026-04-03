//! Process startup: config report and logging.

use std::path::PathBuf;

use crate::args::Args;
use crate::logging;

pub(crate) fn print_config_report() {
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

pub(crate) fn init_logging(args: &Args) -> logging::LogGuard {
    let log_level = args
        .log_level
        .clone()
        .or_else(|| {
            std::env::var("RUST_LOG")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or_else(|| "info".to_string());
    let log_file = args.log_file.clone().or_else(|| {
        std::env::var_os("LOG_FILE")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    });

    let log_args = logging::LogArgs::new(
        log_level,
        log_file,
        &args.log_rotate,
        args.working_folder.clone(),
    );
    logging::init(&log_args)
}
