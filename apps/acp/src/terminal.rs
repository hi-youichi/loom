use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::{Notify, RwLock};
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TerminalSession {
    pub terminal_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub output_byte_limit: Option<u64>,
    pub status: TerminalStatus,
    pub output_buffer: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TerminalStatus {
    Running,
    Completed {
        exit_code: Option<u32>,
        signal: Option<String>,
    },
    Killed,
    Released,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum TerminalError {
    #[error("Terminal not found: {0}")]
    NotFound(String),

    #[error("Failed to create terminal: {0}")]
    CreationFailed(String),

    #[error("Terminal already released: {0}")]
    AlreadyReleased(String),

    #[error("Terminal not running: {0}")]
    NotRunning(String),

    #[error("Terminal wait timed out: {0}")]
    Timeout(String),
}

struct TerminalEntry {
    session: TerminalSession,
    child: Option<Child>,
    pid: Option<u32>,
    stdin: Option<Arc<tokio::sync::Mutex<tokio::process::ChildStdin>>>,
    output_notify: Arc<Notify>,
    exit_notify: Arc<Notify>,
}

pub struct TerminalManager {
    terminals: Arc<RwLock<HashMap<String, TerminalEntry>>>,
    bus: Arc<RwLock<Option<Arc<crate::global_events::GlobalEventBus>>>>,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            terminals: Arc::new(RwLock::new(HashMap::new())),
            bus: Arc::new(RwLock::new(None)),
        }
    }

    /// Attach the global event bus so output/exit events fan out to
    /// subscribed connections (`terminal` topic).
    pub async fn set_bus(&self, bus: Arc<crate::global_events::GlobalEventBus>) {
        *self.bus.write().await = Some(bus);
    }

    #[allow(dead_code)]
    async fn bus(&self) -> Option<Arc<crate::global_events::GlobalEventBus>> {
        self.bus.read().await.clone()
    }

    fn bus_blocking(&self) -> Option<Arc<crate::global_events::GlobalEventBus>> {
        self.bus.try_read().ok().and_then(|guard| guard.clone())
    }

    fn publish_terminal_event(
        &self,
        terminal_id: &str,
        event_type: &str,
        properties: serde_json::Value,
    ) {
        let bus = self.bus_blocking();
        if let Some(bus) = bus {
            bus.publish("terminal", event_type, properties);
            let _ = terminal_id;
        }
    }

    pub async fn create_terminal(
        &self,
        command: String,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        env: Vec<(String, String)>,
        output_byte_limit: Option<u64>,
    ) -> Result<String, TerminalError> {
        let terminal_id = format!("term-{}", Uuid::new_v4());
        info!(
            terminal_id = %terminal_id,
            command = %command,
            args = ?args,
            cwd = ?cwd,
            env_count = env.len(),
            output_byte_limit = ?output_byte_limit,
            "Creating terminal"
        );

        let mut cmd = Command::new(&command);
        cmd.args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if let Some(ref dir) = cwd {
            cmd.current_dir(dir);
        }

        for (k, v) in &env {
            cmd.env(k, v);
        }

        #[cfg(windows)]
        {
            #[allow(unused_imports)]
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| {
                error!(terminal_id = %terminal_id, command = %command, error = %e, "Failed to spawn terminal");
                TerminalError::CreationFailed(e.to_string())
            })?;

        let command_for_event = command.clone();
        let cwd_for_event = cwd.clone();

        let pid = child.id();
        debug!(terminal_id = %terminal_id, pid = ?pid, "Process spawned");
        let stdin = child
            .stdin
            .take()
            .map(|s| Arc::new(tokio::sync::Mutex::new(s)));
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let session = TerminalSession {
            terminal_id: terminal_id.clone(),
            command,
            args,
            cwd,
            env,
            output_byte_limit,
            status: TerminalStatus::Running,
            output_buffer: String::new(),
            truncated: false,
        };

        let output_notify = Arc::new(Notify::new());
        let exit_notify = Arc::new(Notify::new());

        let entry = TerminalEntry {
            session,
            child: Some(child),
            pid,
            stdin,
            output_notify: output_notify.clone(),
            exit_notify: exit_notify.clone(),
        };

        self.terminals
            .write()
            .await
            .insert(terminal_id.clone(), entry);

        if let Some(stdout) = stdout {
            self.spawn_output_reader(terminal_id.clone(), stdout, output_byte_limit);
        }
        if let Some(stderr) = stderr {
            self.spawn_output_reader(terminal_id.clone(), stderr, output_byte_limit);
        }

        self.spawn_exit_watcher(terminal_id.clone());

        self.publish_terminal_event(
            &terminal_id,
            "terminal.created",
            serde_json::json!({
                "terminalId": terminal_id,
                "command": command_for_event,
                "cwd": cwd_for_event,
            }),
        );

        info!(terminal_id = %terminal_id, "Terminal created successfully");
        Ok(terminal_id)
    }

    fn spawn_exit_watcher(&self, terminal_id: String) {
        let terminals = self.terminals.clone();
        let bus = self.bus.clone();
        tokio::spawn(async move {
            let child_process = {
                let mut map = terminals.write().await;
                if let Some(entry) = map.get_mut(&terminal_id) {
                    entry.child.take()
                } else {
                    None
                }
            };

            if let Some(mut child) = child_process {
                const CHILD_WAIT_TIMEOUT: Duration = Duration::from_secs(3600);
                let result = tokio::time::timeout(CHILD_WAIT_TIMEOUT, child.wait()).await;
                let status = match result {
                    Ok(Ok(exit_status)) => {
                        let exit_code = exit_status.code().map(|c| c as u32);
                        #[cfg(unix)]
                        let signal = {
                            use std::os::unix::process::ExitStatusExt;
                            exit_status.signal().map(|s| format!("SIG{}", s))
                        };
                        #[cfg(not(unix))]
                        let signal = None;
                        info!(
                            terminal_id = %terminal_id,
                            exit_code = ?exit_code,
                            signal = ?signal,
                            "Process exited"
                        );
                        TerminalStatus::Completed { exit_code, signal }
                    }
                    Ok(Err(e)) => {
                        error!(terminal_id = %terminal_id, error = %e, "Exit watcher failed to wait for process");
                        TerminalStatus::Completed {
                            exit_code: None,
                            signal: None,
                        }
                    }
                    Err(_) => {
                        warn!(
                            terminal_id = %terminal_id,
                            timeout = ?CHILD_WAIT_TIMEOUT,
                            "Child process did not exit within timeout, killing"
                        );
                        let _ = child.kill().await;
                        TerminalStatus::Killed
                    }
                };

                let mut map = terminals.write().await;
                if let Some(entry) = map.get_mut(&terminal_id) {
                    if matches!(entry.session.status, TerminalStatus::Running) {
                        debug!(terminal_id = %terminal_id, status = "running → completed", "Updating terminal status");
                        entry.session.status = status.clone();
                        entry.exit_notify.notify_waiters();
                        entry.output_notify.notify_waiters();
                        if let Some(bus) = bus.read().await.as_ref() {
                            bus.publish(
                                "terminal",
                                "terminal.exit",
                                serde_json::json!({
                                    "terminalId": terminal_id,
                                    "status": format!("{:?}", status),
                                }),
                            );
                        }
                    }
                } else {
                    warn!(terminal_id = %terminal_id, "Terminal not found when setting exit status");
                }
            }
        });
    }

    fn spawn_output_reader<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
        &self,
        terminal_id: String,
        mut reader: R,
        _output_byte_limit: Option<u64>,
    ) {
        let terminals = self.terminals.clone();
        let bus = self.bus.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]);
                        trace!(terminal_id = %terminal_id, bytes = n, "Output chunk received");
                        let mut appended = false;
                        {
                            let mut map = terminals.write().await;
                            if let Some(entry) = map.get_mut(&terminal_id) {
                                if let Some(limit) = entry.session.output_byte_limit {
                                    let new_len = entry.session.output_buffer.len() + text.len();
                                    if new_len > limit as usize {
                                        entry.session.truncated = true;
                                        trace!(terminal_id = %terminal_id, new_len, limit, "Output truncated (over limit)");
                                        continue;
                                    }
                                }
                                entry.session.output_buffer.push_str(&text);
                                entry.output_notify.notify_waiters();
                                appended = true;
                            }
                        }
                        if appended {
                            if let Some(bus) = bus.read().await.as_ref() {
                                bus.publish(
                                    "terminal",
                                    "terminal.output",
                                    serde_json::json!({
                                        "terminalId": terminal_id,
                                        "chunk": text,
                                    }),
                                );
                            }
                        }
                    }
                    Err(_) => break,
                }
            }

            let mut map = terminals.write().await;
            if let Some(entry) = map.get_mut(&terminal_id) {
                if matches!(entry.session.status, TerminalStatus::Running) {
                    entry.output_notify.notify_waiters();
                }
            }
        });
    }

    pub async fn wait_for_exit(&self, terminal_id: &str) -> Result<TerminalStatus, TerminalError> {
        debug!(terminal_id = %terminal_id, "wait_for_exit called");
        let exit_notify = {
            let map = self.terminals.read().await;
            let entry = map
                .get(terminal_id)
                .ok_or_else(|| TerminalError::NotFound(terminal_id.to_string()))?;
            if matches!(entry.session.status, TerminalStatus::Released) {
                return Err(TerminalError::AlreadyReleased(terminal_id.to_string()));
            }
            if !matches!(entry.session.status, TerminalStatus::Running) {
                return Ok(entry.session.status.clone());
            }
            entry.exit_notify.clone()
        };

        const WAIT_NOTIFY_TIMEOUT: Duration = Duration::from_secs(3700);
        match tokio::time::timeout(WAIT_NOTIFY_TIMEOUT, exit_notify.notified()).await {
            Ok(_) => {}
            Err(_) => {
                warn!(
                    terminal_id = %terminal_id,
                    timeout = ?WAIT_NOTIFY_TIMEOUT,
                    "wait_for_exit timed out waiting for exit notification"
                );
                return Err(TerminalError::Timeout(terminal_id.to_string()));
            }
        }
        debug!(terminal_id = %terminal_id, "wait_for_exit notified");

        let map = self.terminals.read().await;
        let entry = map
            .get(terminal_id)
            .ok_or_else(|| TerminalError::NotFound(terminal_id.to_string()))?;
        let status = entry.session.status.clone();
        info!(terminal_id = %terminal_id, status = ?status, "wait_for_exit completed");
        Ok(status)
    }

    /// Write raw bytes to the terminal process's stdin.
    pub async fn write_input(&self, terminal_id: &str, data: &[u8]) -> Result<(), TerminalError> {
        let stdin = {
            let map = self.terminals.read().await;
            let entry = map
                .get(terminal_id)
                .ok_or_else(|| TerminalError::NotFound(terminal_id.to_string()))?;
            if !matches!(entry.session.status, TerminalStatus::Running) {
                return Err(TerminalError::NotRunning(terminal_id.to_string()));
            }
            entry.stdin.clone()
        };
        let Some(stdin) = stdin else {
            return Err(TerminalError::NotRunning(terminal_id.to_string()));
        };
        let mut guard = stdin.lock().await;
        guard
            .write_all(data)
            .await
            .map_err(|e| TerminalError::NotRunning(e.to_string()))
    }

    pub async fn kill(&self, terminal_id: &str) -> Result<(), TerminalError> {
        info!(terminal_id = %terminal_id, "kill called");
        let mut map = self.terminals.write().await;
        let entry = map
            .get_mut(terminal_id)
            .ok_or_else(|| TerminalError::NotFound(terminal_id.to_string()))?;

        if matches!(entry.session.status, TerminalStatus::Released) {
            return Err(TerminalError::AlreadyReleased(terminal_id.to_string()));
        }

        if let Some(ref mut child) = entry.child {
            let _ = child.kill().await;
        } else if let Some(pid) = entry.pid {
            #[cfg(unix)]
            {
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
            }
            #[cfg(windows)]
            {
                let mut tk = Command::new("taskkill");
                tk.args(["/PID", &pid.to_string(), "/F", "/T"]);
                {
                    #[allow(unused_imports)]
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                    tk.creation_flags(CREATE_NO_WINDOW);
                }
                let _ = tk.output().await;
            }
        }
        entry.session.status = TerminalStatus::Killed;
        info!(terminal_id = %terminal_id, "Terminal killed");
        entry.exit_notify.notify_waiters();
        entry.output_notify.notify_waiters();
        Ok(())
    }

    pub async fn release(&self, terminal_id: &str) -> Result<(), TerminalError> {
        info!(terminal_id = %terminal_id, "release called");
        let mut map = self.terminals.write().await;
        let entry = map
            .get_mut(terminal_id)
            .ok_or_else(|| TerminalError::NotFound(terminal_id.to_string()))?;

        if matches!(entry.session.status, TerminalStatus::Released) {
            return Err(TerminalError::AlreadyReleased(terminal_id.to_string()));
        }

        if let Some(ref mut child) = entry.child {
            let _ = child.kill().await;
        } else if let Some(pid) = entry.pid {
            #[cfg(unix)]
            {
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
            }
            #[cfg(windows)]
            {
                let mut tk = Command::new("taskkill");
                tk.args(["/PID", &pid.to_string(), "/F", "/T"]);
                {
                    #[allow(unused_imports)]
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                    tk.creation_flags(CREATE_NO_WINDOW);
                }
                let _ = tk.output().await;
            }
        }
        entry.child = None;
        entry.session.status = TerminalStatus::Released;
        info!(terminal_id = %terminal_id, "Terminal released");
        entry.exit_notify.notify_waiters();
        entry.output_notify.notify_waiters();
        Ok(())
    }

    pub async fn get_terminal(&self, terminal_id: &str) -> Option<TerminalSession> {
        self.terminals
            .read()
            .await
            .get(terminal_id)
            .map(|e| e.session.clone())
    }

    pub async fn get_status(&self, terminal_id: &str) -> Option<TerminalStatus> {
        self.terminals
            .read()
            .await
            .get(terminal_id)
            .map(|e| e.session.status.clone())
    }

    pub async fn get_output(
        &self,
        terminal_id: &str,
    ) -> Option<(String, bool, Option<TerminalStatus>)> {
        let result = self.terminals.read().await.get(terminal_id).map(|e| {
            let output_len = e.session.output_buffer.len();
            let truncated = e.session.truncated;
            let status = if !matches!(e.session.status, TerminalStatus::Running) {
                Some(e.session.status.clone())
            } else {
                None
            };
            debug!(
                terminal_id = %terminal_id,
                output_len,
                truncated,
                has_exit_status = status.is_some(),
                "get_output"
            );
            (e.session.output_buffer.clone(), truncated, status)
        });
        if result.is_none() {
            warn!(terminal_id = %terminal_id, "get_output: terminal not found");
        }
        result
    }

    pub async fn append_output(&self, terminal_id: &str, output: &str) {
        if let Some(entry) = self.terminals.write().await.get_mut(terminal_id) {
            entry.session.output_buffer.push_str(output);
        }
    }

    pub async fn update_status(&self, terminal_id: &str, status: TerminalStatus) {
        if let Some(entry) = self.terminals.write().await.get_mut(terminal_id) {
            entry.session.status = status;
        }
    }
}

impl Clone for TerminalManager {
    fn clone(&self) -> Self {
        Self {
            terminals: Arc::clone(&self.terminals),
            bus: Arc::clone(&self.bus),
        }
    }
}
