//! ACP server entrypoint: PID management, panic hook, SIGHUP reload.
//!
//! Extracted from the former `loom-acp` binary so the `loom acp` subcommand
//! can reuse all the same machinery without a separate executable.

use std::path::PathBuf;

use crate::logging::LogConfig;
use crate::run_stdio_loop;

/// Exit code used when the server shuts down for SIGHUP-triggered reload.
/// The caller (IDE / wrapper script) can check this code to restart.
pub const RELOAD_EXIT_CODE: i32 = 203;

// ---------------------------------------------------------------------------
// Public helpers
// ---------------------------------------------------------------------------

/// Returns `~/.loom/acp` (or `$LOOM_HOME/acp`) as the log/PID directory.
pub fn acp_log_dir() -> Option<PathBuf> {
    Some(config::home::acp_data_dir())
}

/// Path to the PID file (`~/.loom/acp/loom-acp.pid`).
fn acp_pid_path() -> Option<PathBuf> {
    acp_log_dir().map(|d| d.join("loom-acp.pid"))
}

// ---------------------------------------------------------------------------
// Reload subcommand
// ---------------------------------------------------------------------------

/// Send SIGHUP to the running ACP process (read PID from `~/.loom/acp`). Unix only.
pub fn run_reload() {
    let pid_path = match acp_pid_path() {
        Some(p) => p,
        None => {
            eprintln!("loom acp reload: could not determine PID file path");
            std::process::exit(1);
        }
    };
    let content = match std::fs::read_to_string(&pid_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "loom acp reload: failed to read PID file {}: {}",
                pid_path.display(),
                e
            );
            std::process::exit(1);
        }
    };
    let pid_str = content.trim().lines().next().unwrap_or("").trim();
    let pid: i32 = match pid_str.parse() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("loom acp reload: invalid PID in {}", pid_path.display());
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
                eprintln!("loom acp reload: kill -HUP {} failed with {}", pid, s);
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("loom acp reload: failed to run kill: {}", e);
                std::process::exit(1);
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        eprintln!("loom acp reload: not supported on this platform");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Server entrypoint
// ---------------------------------------------------------------------------

/// Run the ACP stdio server.
///
/// Sets up panic hook, loads config silently, initializes logging, writes PID
/// file, then runs [`run_stdio_loop`] until stdin closes or SIGHUP is received
/// (unix only).
pub async fn run_server(
    log_config: LogConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Panic hook — log and print before the default handler aborts.
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
        eprintln!("loom acp panic: {} at {}", msg, location);
        tracing::error!(location = %location, msg = %msg, "panic caught");
        default_hook(info);
    }));

    // Config load — silent (no stdout output, unlike CLI's print_config_report).
    let _ = config::load_and_apply_with_report("loom", None::<&std::path::Path>).ok();

    // Logging — delayed init via OnceLock.
    crate::set_log_config(log_config);

    // PID file guard — removed on drop (normal exit).
    let _pid_guard = write_pid_file(&acp_log_dir());

    // Run stdio loop, with SIGHUP → reload on unix.
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sig = match signal(SignalKind::hangup()) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to install SIGHUP handler: {e}");
                return Err(e.into());
            }
        };
        tokio::select! {
            res = run_stdio_loop() => handle_stdio_result(res),
            _ = sig.recv() => {
                tracing::info!("SIGHUP received, exiting for reload");
                std::process::exit(RELOAD_EXIT_CODE);
            }
        }
    }

    #[cfg(not(unix))]
    {
        let result = run_stdio_loop().await;
        handle_stdio_result(result)
    }
}

fn handle_stdio_result(
    result: Result<crate::StdioLoopResult, Box<dyn std::error::Error + Send + Sync>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match result {
        Ok(r) => {
            if r.connection_closed {
                tracing::info!("ACP server exiting: client connection closed");
            } else {
                tracing::info!("ACP server exiting normally");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("loom acp fatal error: {}", e);
            eprintln!(
                "Hint: check logs at ~/.loom/acp/loom-acp.log or run with --log-level debug"
            );
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// PID file management
// ---------------------------------------------------------------------------

/// Removes the PID file on drop (normal exit).
struct PidFileGuard(Option<PathBuf>);

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Writes current process PID to the given directory and returns a guard.
fn write_pid_file(log_dir: &Option<PathBuf>) -> Option<PidFileGuard> {
    let dir = log_dir.as_ref()?;
    std::fs::create_dir_all(dir).ok()?;
    let path = dir.join("loom-acp.pid");
    let pid = std::process::id();
    std::fs::write(&path, format!("{}\n", pid)).ok()?;
    tracing::info!(pid = pid, pid_file = %path.display(), "ACP PID file written");
    Some(PidFileGuard(Some(path)))
}
