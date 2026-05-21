//! # loom-acp binary entrypoint
//!
//! IDEs (Zed, JetBrains, etc.) configure this executable as the ACP Agent command and communicate
//! over stdio. On startup we load [config](config) (same as loom), set log config for delayed init,
//! write a PID file, then run [`loom_acp::run_stdio_loop`] until stdin
//! closes or an error occurs. SIGHUP triggers reload (exit with code 203 so the caller can restart).
//!
//! Subcommand `reload`: sends SIGHUP to the process whose PID is in the PID file (Unix only).

use std::path::PathBuf;
use std::str::FromStr;

pub const RELOAD_EXIT_CODE: i32 = 203;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Args = clap::Parser::parse();

    if args.show_log_dir {
        match acp_log_dir() {
            Some(p) => println!("{}", p.display()),
            None => {
                eprintln!("loom-acp: could not determine log directory");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    if let Some(Cmd::Reload) = args.cmd {
        run_reload();
        return Ok(());
    }

    run_server(args)
}

const HELP_LOG_DIR: &str = "\nLog and PID directory: ~/.loom/acp (or $LOOM_HOME/acp). \
Use --show-log-dir to print the actual path for this environment.";

#[derive(clap::Parser, Debug)]
#[command(name = "loom-acp", after_help = HELP_LOG_DIR)]
struct Args {
    /// Print the log (and PID) file directory and exit.
    #[arg(long)]
    show_log_dir: bool,

    /// Log level: trace, debug, info, warn, error
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Log file path. Relative paths are resolved against the session working_folder. Default: ~/.loom/acp/loom-acp.log
    #[arg(long, value_name = "PATH")]
    log_file: Option<PathBuf>,

    /// Log rotation strategy: none, daily, hourly, minutely
    #[arg(long, default_value = "daily", value_name = "STRATEGY")]
    log_rotate: String,

    /// Log output format: text (default) or json
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    log_format: String,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(clap::Subcommand, Debug)]
enum Cmd {
    /// Send SIGHUP to the running loom-acp process (read PID from `~/.loom/acp`). Unix only.
    Reload,
}

fn run_reload() {
    let pid_path = match acp_pid_path() {
        Some(p) => p,
        None => {
            eprintln!("loom-acp reload: could not determine PID file path");
            std::process::exit(1);
        }
    };
    let content = match std::fs::read_to_string(&pid_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "loom-acp reload: failed to read PID file {}: {}",
                pid_path.display(),
                e
            );
            std::process::exit(1);
        }
    };
    let pid_str = content.trim().lines().next().unwrap_or("").trim();
    let _pid: i32 = match pid_str.parse() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("loom-acp reload: invalid PID in {}", pid_path.display());
            std::process::exit(1);
        }
    };

    #[cfg(unix)]
    {
        let status = std::process::Command::new("kill")
            .arg("-HUP")
            .arg(_pid.to_string())
            .status();
        match status {
            Ok(s) if s.success() => std::process::exit(0),
            Ok(s) => {
                eprintln!("loom-acp reload: kill -HUP {} failed with {}", _pid, s);
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("loom-acp reload: failed to run kill: {}", e);
                std::process::exit(1);
            }
        }
    }

    #[cfg(not(unix))]
    {
        eprintln!("loom-acp reload: not supported on this platform");
        std::process::exit(1);
    }
}

fn run_server(args: Args) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        eprintln!("loom-acp panic: {} at {}", msg, location);
        tracing::error!(location = %location, msg = %msg, "panic caught");
        default_hook(info);
    }));

    let _ = config::load_and_apply_with_report("loom", None::<&std::path::Path>).ok();

    loom_acp::set_log_config(loom_acp::logging::LogConfig {
        level: args.log_level.clone(),
        file: args.log_file.clone(),
        rotate: config::tracing_init::LogRotate::from_str_or_daily(&args.log_rotate),
        format: loom_acp::logging::LogFormat::from_str(&args.log_format).unwrap_or_default(),
    });

    let _pid_guard = write_pid_file(&acp_log_dir());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;

    #[cfg(unix)]
    let result = {
        use tokio::signal::unix::{signal, SignalKind};
        rt.block_on(async {
            let mut sig = match signal(SignalKind::hangup()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to install SIGHUP handler: {e}");
                    return Err(e.into());
                }
            };
            tokio::select! {
                res = loom_acp::run_stdio_loop() => res,
                _ = sig.recv() => {
                    tracing::info!("SIGHUP received, exiting for reload");
                    Err(format!("reload (exit {})", RELOAD_EXIT_CODE).into())
                }
            }
        })
    };

    #[cfg(not(unix))]
    let result = rt.block_on(loom_acp::run_stdio_loop());

    match result {
        Ok(r) => {
            if r.connection_closed {
                tracing::info!("loom-acp exiting: client connection closed");
            } else {
                tracing::info!("loom-acp exiting normally");
            }
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains(&format!("reload (exit {}", RELOAD_EXIT_CODE)) {
                tracing::info!("Exiting for reload, PidFileGuard will clean up");
                std::process::exit(RELOAD_EXIT_CODE);
            }
            eprintln!("loom-acp fatal error: {}", msg);
            eprintln!("Hint: check logs at ~/.loom/acp/loom-acp.log or run with --log-level debug");
            Err(e)
        }
    }
}

/// Removes the PID file on drop (normal exit or reload).
struct PidFileGuard(Option<PathBuf>);

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Writes current process PID to `~/.loom/acp` and returns a guard that removes it on drop.
fn write_pid_file(log_dir: &Option<PathBuf>) -> Option<PidFileGuard> {
    let dir = log_dir.as_ref()?;
    std::fs::create_dir_all(dir).ok()?;
    let path = dir.join("loom-acp.pid");
    let pid = std::process::id();
    std::fs::write(&path, format!("{}\n", pid)).ok()?;
    tracing::info!(pid = pid, pid_file = %path.display(), "ACP PID file written");
    Some(PidFileGuard(Some(path)))
}

/// Returns `~/.loom/acp` as log/PID directory.
fn acp_log_dir() -> Option<PathBuf> {
    Some(config::home::acp_data_dir())
}

/// Path to the PID file (`~/.loom/acp/loom-acp.pid`).
fn acp_pid_path() -> Option<PathBuf> {
    acp_log_dir().map(|d| d.join("loom-acp.pid"))
}
