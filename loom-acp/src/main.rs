//! # loom-acp binary entrypoint
//!
//! IDEs (Zed, JetBrains, etc.) configure this executable as the ACP Agent command and communicate
//! over stdio. On startup we load [config](config) (same as loom), init tracing (stderr + XDG log
//! directory), write a PID file under XDG state, then run [`loom_acp::run_stdio_loop`] until stdin
//! closes or an error occurs. SIGHUP triggers reload (exit with code 203 so the caller can restart).
//!
//! Subcommand `reload`: sends SIGHUP to the process whose PID is in the PID file (Unix only).

use std::path::PathBuf;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Exit code used when exiting for reload (caller should restart the process).
pub const RELOAD_EXIT_CODE: i32 = 203;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Args = clap::Parser::parse();

    if args.show_log_dir {
        match acp_log_dir() {
            Some(p) => println!("{}", p.display()),
            None => {
                eprintln!("loom-acp: could not determine log directory (XDG state dir missing)");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    if let Some(Cmd::Reload) = args.cmd {
        run_reload();
        return Ok(());
    }

    run_server()
}

const HELP_LOG_DIR: &str = "\nLog and PID directory: $XDG_STATE_HOME/loom-acp (e.g. ~/.local/state/loom-acp on Linux). \
Use --show-log-dir to print the actual path for this environment.";

#[derive(clap::Parser, Debug)]
#[command(name = "loom-acp", after_help = HELP_LOG_DIR)]
struct Args {
    /// Print the log (and PID) file directory and exit.
    #[arg(long)]
    show_log_dir: bool,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(clap::Subcommand, Debug)]
enum Cmd {
    /// Send SIGHUP to the running loom-acp process (read PID from XDG state). Unix only.
    Reload,
}

fn run_reload() {
    let pid_path = match acp_pid_path() {
        Some(p) => p,
        None => {
            eprintln!("loom-acp reload: could not determine PID file path (XDG state dir missing)");
            std::process::exit(1);
        }
    };
    let content = match std::fs::read_to_string(&pid_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("loom-acp reload: failed to read PID file {}: {}", pid_path.display(), e);
            std::process::exit(1);
        }
    };
    let pid_str = content.trim().lines().next().unwrap_or("").trim();
    let pid: i32 = match pid_str.parse() {
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
            .arg(pid.to_string())
            .status();
        match status {
            Ok(s) if s.success() => std::process::exit(0),
            Ok(s) => {
                eprintln!("loom-acp reload: kill -HUP {} failed with {}", pid, s);
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

fn run_server() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    config::load_and_apply("loom", None::<&std::path::Path>).ok();

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_stderr = tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .with_writer(std::io::stderr);

    let log_dir = acp_log_dir();
    let _guard: Option<Box<tracing_appender::non_blocking::WorkerGuard>> =
        if let Some(dir) = &log_dir {
            if std::fs::create_dir_all(dir).is_ok() {
                if let Ok(file_appender) =
                    tracing_appender::rolling::RollingFileAppender::builder()
                        .rotation(tracing_appender::rolling::Rotation::DAILY)
                        .filename_prefix("loom-acp")
                        .filename_suffix("log")
                        .build(dir)
                {
                    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
                    let fmt_file = tracing_subscriber::fmt::layer()
                        .with_ansi(false)
                        .with_writer(non_blocking);
                    tracing_subscriber::registry()
                        .with(env_filter)
                        .with(fmt_stderr)
                        .with(fmt_file)
                        .init();
                    Some(Box::new(guard))
                } else {
                    tracing_subscriber::registry()
                        .with(env_filter)
                        .with(fmt_stderr)
                        .init();
                    None
                }
            } else {
                tracing_subscriber::registry()
                    .with(env_filter)
                    .with(fmt_stderr)
                    .init();
                None
            }
        } else {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_stderr)
                .init();
            None
        };

    if let Some(ref d) = log_dir {
        tracing::info!(log_dir = %d.display(), "ACP log directory");
    }

    let _pid_guard = write_pid_file(&log_dir);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    #[cfg(unix)]
    let run = async {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sig = match signal(SignalKind::hangup()) {
            Ok(s) => s,
            Err(_) => return loom_acp::run_stdio_loop().await,
        };
        tokio::select! {
            res = loom_acp::run_stdio_loop() => res,
            _ = sig.recv() => {
                tracing::info!("SIGHUP received, exiting for reload");
                std::process::exit(RELOAD_EXIT_CODE);
            }
        }
    };

    #[cfg(not(unix))]
    let run = loom_acp::run_stdio_loop();

    rt.block_on(run)
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

/// Writes current process PID to XDG state dir and returns a guard that removes it on drop.
fn write_pid_file(log_dir: &Option<PathBuf>) -> Option<PidFileGuard> {
    let dir = log_dir.as_ref()?;
    std::fs::create_dir_all(dir).ok()?;
    let path = dir.join("loom-acp.pid");
    let pid = std::process::id();
    std::fs::write(&path, format!("{}\n", pid)).ok()?;
    tracing::info!(pid = pid, pid_file = %path.display(), "ACP PID file written");
    Some(PidFileGuard(Some(path)))
}

/// Returns XDG state home directory for loom-acp (e.g. ~/.local/state/loom-acp on Linux).
/// Used as the parent for rolling log files and the PID file.
fn acp_log_dir() -> Option<PathBuf> {
    cross_xdg::BaseDirs::with_prefix("loom-acp")
        .ok()
        .map(|d| d.state_home().to_path_buf())
}

/// Path to the PID file (loom-acp.pid in XDG state dir).
fn acp_pid_path() -> Option<PathBuf> {
    acp_log_dir().map(|d| d.join("loom-acp.pid"))
}
