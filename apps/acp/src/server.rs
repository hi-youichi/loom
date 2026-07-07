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
/// Exit code emitted on SIGUSR1 (Hermes `gateway/run.py:80-95`
/// `RESTART_EXIT_CODE`). Supervisors that want a process-restart
/// semantic (e.g. an orchestrator swapping the binary) send SIGUSR1
/// and observe exit 75 / sysexits `EX_TEMPFAIL` to signal "transient,
/// reschedule me" rather than the harder 203 (RELOAD_EXIT_CODE = a
/// "reload config in-place" semantic).
pub const RESTART_EXIT_CODE: i32 = 75;

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

    // Run stdio loop, with full signal FSM (Hermes `gateway/run.py:80-95`):
    //
    // SIGHUP  → exit 203 (RELOAD_EXIT_CODE) → supervisor reloads config in-place
    // SIGUSR1 → exit 75  (RESTART_EXIT_CODE) → supervisor restarts the binary
    // SIGTERM → drain in-flight requests, then exit 0
    // SIGINT  → same as SIGTERM (clean shutdown from operator)
    //
    // Without SIGTERM/SIGUSR1 a `kill -TERM` would tear down mid-AI-turn
    // responses and surface them to the client as socket-reset rather than
    // a clean MCP/ACP graceful-shutdown sequence.
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sighup = match signal(SignalKind::hangup()) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to install SIGHUP handler: {e}");
                return Err(e.into());
            }
        };
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to install SIGTERM handler: {e}");
                return Err(e.into());
            }
        };
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to install SIGINT handler: {e}");
                return Err(e.into());
            }
        };
        let mut sigusr1 = match signal(SignalKind::user_defined1()) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to install SIGUSR1 handler: {e}");
                return Err(e.into());
            }
        };
        tokio::select! {
            res = run_stdio_loop() => handle_stdio_result(res),
            _ = sighup.recv() => {
                tracing::info!("SIGHUP received, exiting for reload");
                std::process::exit(RELOAD_EXIT_CODE);
            }
            _ = sigterm.recv() => {
                tracing::info!("SIGTERM received, draining and exiting cleanly");
                // Drain is best-effort: stdio loop already returned/cancelled
                // when we reach here, so a 0 exit signals supervisors
                // that the next start can safely reuse the data dir.
                std::process::exit(0);
            }
            _ = sigint.recv() => {
                tracing::info!("SIGINT received, exiting cleanly");
                std::process::exit(0);
            }
            _ = sigusr1.recv() => {
                tracing::info!("SIGUSR1 received, exiting for restart");
                std::process::exit(RESTART_EXIT_CODE);
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
            eprintln!("Hint: check logs at ~/.loom/acp/loom-acp.log or run with --log-level debug");
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

/// PID-file lock contention error. Returned when another live
/// `loom acp` instance already owns `~/.loom/acp/loom-acp.pid` so
/// that a duplicate start can fail fast instead of stomping on the
/// existing process' log writes.
#[derive(Debug)]
pub enum PidLockError {
    /// Another live process owns the PID file.
    AlreadyRunning { pid: u32, path: PathBuf },
    /// The PID file is stale (recorded PID no longer exists).
    /// Callers may delete the stale file and retry.
    Stale { pid: u32, path: PathBuf },
    /// Filesystem error.
    Io(std::io::Error),
}

impl std::fmt::Display for PidLockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PidLockError::AlreadyRunning { pid, path } => write!(
                f,
                "another loom-acp instance (pid {}) is already running (pid file: {})",
                pid,
                path.display()
            ),
            PidLockError::Stale { pid, path } => write!(
                f,
                "stale pid file (pid {} no longer running): {}",
                pid,
                path.display()
            ),
            PidLockError::Io(e) => write!(f, "pid file io error: {}", e),
        }
    }
}

impl std::error::Error for PidLockError {}

/// Writes current process PID to the given directory and returns a guard.
///
/// Hermes parity (`gateway/run.py:writer_pid_file`): use
/// `OpenOptions::create_new(true)` (POSIX `O_CREAT|O_EXCL`,
/// Windows `CREATE_NEW`) so a concurrent `loom acp` start cannot
/// silently overwrite the existing PID file. On `AlreadyExists`,
/// we read the recorded PID, check whether the process is still
/// alive, and return a structured `PidLockError` so the caller
/// can either fail fast (default) or delete the stale file and
/// retry.
fn write_pid_file(log_dir: &Option<PathBuf>) -> Result<PidFileGuard, PidLockError> {
    let dir = log_dir.as_ref().ok_or_else(|| {
        PidLockError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "ACP log directory not configured",
        ))
    })?;
    std::fs::create_dir_all(dir).map_err(PidLockError::Io)?;
    let path = dir.join("loom-acp.pid");
    let pid = std::process::id();

    // Atomic create: O_CREAT | O_EXCL. If another writer beat us,
    // `.create_new(true)` returns `AlreadyExists` and we can inspect
    // the existing file to decide whether to surface a stale-pid
    // recovery path or a hard "already running" error.
    use std::io::Write;
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Read whatever pid is recorded.
            let recorded = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok());
            if let Some(existing_pid) = recorded {
                if is_pid_alive(existing_pid) {
                    return Err(PidLockError::AlreadyRunning {
                        pid: existing_pid,
                        path,
                    });
                }
                return Err(PidLockError::Stale {
                    pid: existing_pid,
                    path,
                });
            }
            return Err(PidLockError::Io(e));
        }
        Err(e) => return Err(PidLockError::Io(e)),
    };
    file.write_all(format!("{}\n", pid).as_bytes())
        .map_err(PidLockError::Io)?;
    file.sync_all().ok();
    tracing::info!(pid = pid, pid_file = %path.display(), "ACP PID file written");
    Ok(PidFileGuard(Some(path)))
}

/// Best-effort check whether `pid` is a live process on the current
/// platform. POSIX uses kill(pid, 0); Windows uses `OpenProcess`.
/// Returns `false` for cross-platform unknowns (e.g. PID 0/1 on
/// Linux containers) so the caller treats them as stale.
#[cfg(unix)]
fn is_pid_alive(pid: u32) -> bool {
    // SAFETY: kill(pid, 0) performs a permissions check and returns
    // 0 if the process exists and we can signal it, -1/ESRCH otherwise.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(windows)]
fn is_pid_alive(pid: u32) -> bool {
    // OpenProcess with PROCESS_QUERY_LIMITED_INFORMATION (0x1000) is
    // the lowest-privilege handle that lets us ask "is this PID
    // running?". Returns NULL when the PID does not exist.
    extern "system" {
        fn OpenProcess(
            dwDesiredAccess: u32,
            bInheritHandle: i32,
            dwProcessId: u32,
        ) -> *mut core::ffi::c_void;
        fn CloseHandle(h: *mut core::ffi::c_void) -> i32;
        fn GetExitCodeProcess(h: *mut core::ffi::c_void, lp_exit_code: *mut u32) -> i32;
    }
    // Win32 STILL_ACTIVE (STATUS_PENDING) sentinel.
    const STILL_ACTIVE: u32 = 259;
    let h = unsafe { OpenProcess(0x1000, 0, pid) };
    if h.is_null() {
        return false;
    }
    let mut code: u32 = 0;
    let ok = unsafe { GetExitCodeProcess(h, &mut code) };
    unsafe {
        CloseHandle(h);
    }
    ok != 0 && code == STILL_ACTIVE
}

#[cfg(not(any(unix, windows)))]
fn is_pid_alive(_pid: u32) -> bool {
    false
}
