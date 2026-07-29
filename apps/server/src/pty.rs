//! Minimal PTY session manager backed by `portable-pty`.
//!
//! Transport-free core: spawns pseudo-terminals, buffers their raw output
//! bytes on a background thread, and exposes a small CRUD + I/O surface.
//! No axum, no HTTP — a handler layer drains the output buffer and forwards
//! input via this manager.
//!
//! Mirrors the opencode PTY contract (`.loom/contract/schema-pty.ts`):
//!   * `Pty.ID`            = `"pty_" + ascending()`   → [`PtyManager::next_id`]
//!   * `Pty.Info`          = `{ id, title, command, args, cwd, status, pid, exitCode? }`
//!   * `Pty.CreateInput`   = `{ command?, args?, cwd?, title?, env?, size? }`
//!   * `Pty.UpdateInput`   = `{ title?, size? { rows, cols } }`
//!   * `status`            ∈ `{ "running", "exited" }`

use parking_lot::{Mutex, RwLock};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;

/// Default viewport for a freshly created PTY when `CreateInput.size` is
/// absent (schema-pty.ts makes the size optional).
const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;

/// Output read granularity for the background drain loop.
const READ_CHUNK: usize = 4096;

/// Lifecycle of a PTY session (schema-pty.ts: `status: "running" | "exited"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyStatus {
    Running,
    Exited,
}

impl PtyStatus {
    /// Wire string used by the opencode contract.
    pub fn as_str(self) -> &'static str {
        match self {
            PtyStatus::Running => "running",
            PtyStatus::Exited => "exited",
        }
    }
}

/// `Pty.Info` (schema-pty.ts). `exit_code` is `None` while running and
/// `Some(code)` after the child exits (serialized as `exitCode`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PtyInfo {
    pub id: String,
    pub title: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    /// `"running"` or `"exited"` (see [`PtyStatus`]).
    pub status: String,
    pub pid: u32,
    #[serde(rename = "exitCode", skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
}

/// Optional initial viewport for [`CreateInput`] (schema-pty.ts
/// `UpdateInput.size` / `CreateInput.size`).
#[derive(Debug, Clone, Copy)]
pub struct PtySizeInput {
    pub rows: u16,
    pub cols: u16,
}

/// `Pty.CreateInput` (schema-pty.ts). All fields optional; `command` absent
/// → the platform default shell is spawned (`$SHELL` / `$COMSPEC`).
#[derive(Debug, Default, Clone)]
pub struct CreateInput {
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub size: Option<PtySizeInput>,
}

/// A live PTY session. The `portable_pty` master is `Send` but not `Sync`,
/// so it lives behind a `Mutex`; the child likewise. `buffer` holds the raw
/// output bytes appended by the background reader (drained via
/// [`PtyManager::drain_output`]). `info` is the mutable metadata returned to
/// callers (status/exit_code flip when the child exits).
struct Session {
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    buffer: Mutex<Vec<u8>>,
    info: RwLock<PtyInfo>,
}

impl Session {
    /// Snapshot the contract [`PtyInfo`] for this session.
    fn info(&self) -> PtyInfo {
        self.info.read().clone()
    }
}

/// Minimal PTY session manager: a map of `ptyID -> Arc<Session>` plus the
/// ascending id generator (`pty_<n>`). All public methods are synchronous and
/// non-blocking; output buffering runs on a background thread per session.
#[derive(Default)]
pub struct PtyManager {
    sessions: RwLock<HashMap<String, Arc<Session>>>,
    counter: Mutex<u64>,
}

impl PtyManager {
    /// Create an empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint the next `pty_<n>` id (schema-pty.ts: `Pty.ID = "pty_" + ascending()`).
    fn next_id(&self) -> String {
        let mut g = self.counter.lock();
        let n = *g;
        *g += 1;
        format!("pty_{n}")
    }

    /// Spawn a PTY for `input`, buffering its output in the background. If
    /// `command` is omitted the platform default shell is used. Returns the
    /// new `pty_<n>` id. Recorded `info.command` / `info.cwd` reflect the
    /// resolved program / working directory.
    pub fn create(&self, input: CreateInput) -> std::io::Result<String> {
        let id = self.next_id();
        let rows = input.size.map(|s| s.rows).unwrap_or(DEFAULT_ROWS);
        let cols = input.size.map(|s| s.cols).unwrap_or(DEFAULT_COLS);

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(pty_err)?;

        // Build the command: explicit program (with optional args) or the
        // platform default shell. `args` are only honored alongside a command.
        let program = input.command.clone().unwrap_or_else(default_shell);
        let mut cmd = CommandBuilder::new(&program);
        if let Some(args) = &input.args {
            for a in args {
                cmd.arg(a);
            }
        }
        if let Some(cwd) = &input.cwd {
            cmd.cwd(cwd);
        }
        if let Some(env) = &input.env {
            for (k, v) in env {
                cmd.env(k, v);
            }
        }

        // Spawn on the slave, then drop the slave so the master reader can
        // observe EOF when the child exits (dropping the slave avoids the
        // deadlock documented by portable-pty when waiting on the child).
        let slave = pair.slave;
        let child = slave.spawn_command(cmd).map_err(pty_err)?;
        drop(slave);

        let pid = child.process_id().unwrap_or(0);
        let args_vec = input.args.clone().unwrap_or_default();
        let cwd_str = input.cwd.clone().unwrap_or_else(current_dir_string);
        let info = PtyInfo {
            id: id.clone(),
            title: input.title.clone().unwrap_or_else(|| program.clone()),
            command: program,
            args: args_vec,
            cwd: cwd_str,
            status: PtyStatus::Running.as_str().to_string(),
            pid,
            exit_code: None,
        };

        let reader = pair.master.try_clone_reader().map_err(pty_err)?;
        let session = Arc::new(Session {
            master: Mutex::new(pair.master),
            child: Mutex::new(child),
            buffer: Mutex::new(Vec::new()),
            info: RwLock::new(info),
        });

        // Background drain: append output bytes until EOF, then record the
        // exit code and flip status to "exited". Holds its own Arc so the
        // session outlives removal from the map until output is drained.
        let drain_session = Arc::clone(&session);
        std::thread::spawn(move || drain_loop(drain_session, reader));

        self.sessions.write().insert(id.clone(), session);
        Ok(id)
    }

    /// Snapshot every session's [`PtyInfo`].
    pub fn list(&self) -> Vec<PtyInfo> {
        self.sessions.read().values().map(|s| s.info()).collect()
    }

    /// Snapshot one session's [`PtyInfo`].
    pub fn get(&self, id: &str) -> Option<PtyInfo> {
        self.sessions.read().get(id).map(|s| s.info())
    }

    /// Drain and return all buffered output bytes for `id`, clearing the
    /// buffer. Returns an empty vec for unknown ids. A handler polls this to
    /// stream a session's output to a client.
    pub fn drain_output(&self, id: &str) -> Vec<u8> {
        self.sessions
            .read()
            .get(id)
            .map(|s| std::mem::take(&mut *s.buffer.lock()))
            .unwrap_or_default()
    }

    /// Update the title of a PTY session (schema-pty.ts `UpdateInput.title`).
    pub fn set_title(&self, id: &str, title: &str) -> std::io::Result<()> {
        let session = self.session(id)?;
        session.info.write().title = title.to_string();
        Ok(())
    }

    /// Resize the PTY viewport (schema-pty.ts `UpdateInput.size`).
    pub fn update_size(&self, id: &str, rows: u16, cols: u16) -> std::io::Result<()> {
        let session = self.session(id)?;
        // Bind the owned result so the `MutexGuard` temporary drops before
        // `session`, avoiding a temporary-lifetime error at block end.
        let result = session.master.lock().resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        result.map_err(pty_err)
    }

    /// Send raw bytes to the PTY's stdin.
    pub fn write_input(&self, id: &str, bytes: &[u8]) -> std::io::Result<()> {
        let session = self.session(id)?;
        let mut writer = session.master.lock().take_writer().map_err(pty_err)?;
        writer.write_all(bytes)
    }

    /// Kill the child (if still running) and drop the session from the map.
    /// Returns `true` if a session was removed. The background reader keeps
    /// draining until EOF; the session is freed once the reader's `Arc`
    /// (the last reference) finishes.
    pub fn remove(&self, id: &str) -> bool {
        let session = self.sessions.write().remove(id);
        if let Some(session) = &session {
            // Best-effort kill so the reader thread reaches EOF promptly.
            let _ = session.child.lock().kill();
        }
        session.is_some()
    }

    /// Look up a session by id or return a NotFound I/O error.
    fn session(&self, id: &str) -> std::io::Result<Arc<Session>> {
        self.sessions
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| not_found(id))
    }
}

/// Read from `reader` into `session.buffer` until EOF, then record the exit
/// code and flip `status` to `"exited"`. Runs on a dedicated thread per
/// session; an interrupted read is retried, any other I/O error ends the loop.
fn drain_loop(session: Arc<Session>, mut reader: Box<dyn Read + Send>) {
    let mut buf = [0u8; READ_CHUNK];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break, // EOF — child output pipe drained
            Ok(n) => session.buffer.lock().extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    // The child has exited (its output stream hit EOF); capture the code.
    let exit_code = match session.child.lock().wait() {
        Ok(status) => Some(status.exit_code()),
        Err(_) => None,
    };
    let mut info = session.info.write();
    info.status = PtyStatus::Exited.as_str().to_string();
    info.exit_code = exit_code;
}

/// Resolve the platform default shell (`$SHELL` on Unix, `$COMSPEC` on
/// Windows) with a sane fallback. Used when `CreateInput.command` is absent.
fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

/// Best-effort current working directory as a string (recorded in
/// [`PtyInfo::cwd`] when `CreateInput.cwd` is absent).
fn current_dir_string() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_default()
}

/// Map a `portable-pty` error (`anyhow::Error`, not a direct dependency) to
/// `std::io::Error` via its `Display` impl — the error type is never named.
fn pty_err<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

/// `NotFound` I/O error for an unknown pty id.
fn not_found(id: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("pty session not found: {id}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PtyStatus` wire strings must match the opencode contract literals.
    #[test]
    fn status_strings_match_contract() {
        assert_eq!(PtyStatus::Running.as_str(), "running");
        assert_eq!(PtyStatus::Exited.as_str(), "exited");
    }

    /// `PtyInfo` must serialize to the schema-pty.ts shape, omitting
    /// `exitCode` while running and including it after exit.
    #[test]
    fn pty_info_serializes_to_contract_shape() {
        let running = PtyInfo {
            id: "pty_0".into(),
            title: "sh".into(),
            command: "sh".into(),
            args: vec![],
            cwd: "/tmp".into(),
            status: "running".into(),
            pid: 123,
            exit_code: None,
        };
        let json = serde_json::to_value(&running).unwrap();
        assert_eq!(json["id"], "pty_0");
        assert_eq!(json["status"], "running");
        assert_eq!(json["pid"], 123);
        assert!(json.get("exitCode").is_none(), "running: no exitCode");

        let exited = PtyInfo {
            status: "exited".into(),
            exit_code: Some(0),
            ..running
        };
        let json = serde_json::to_value(&exited).unwrap();
        assert_eq!(json["status"], "exited");
        assert_eq!(json["exitCode"], 0);
    }
}
