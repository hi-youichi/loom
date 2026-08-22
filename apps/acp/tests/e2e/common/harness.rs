//! `AcpTestHarness` — spawn the real `loom-acp` binary and own its stdio.
//!
//! Plan 026 §2.1. The harness owns:
//! - The child process + its stdin/stdout/stderr pipes.
//! - A `JsonRpcClient` for typed request/response/notification plumbing.
//! - A `ReverseRpcResponder` that auto-answers agent→client RPCs.
//!
//! Writing to stdin is funnelled through a `tokio::sync::mpsc::UnboundedSender`
//! so both the test function and the background reader (for reverse-RPC replies)
//! can write without holding `&mut self`.

#![allow(dead_code)]

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::common::env::{binary_path, TestEnv};
use crate::common::jsonrpc::{JsonRpcClient, JsonRpcFrame, SessionNotification};
use crate::common::permissions::ReverseRpcResponder;

const GRACEFUL_EXIT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct AcpTestHarness {
    child: Child,
    /// Channel sender for writing JSON-RPC frames to the child's stdin.
    /// The writer task drains this and writes to the actual pipe.
    write_tx: Option<mpsc::UnboundedSender<String>>,
    /// Background reader task handle — aborted on shutdown so its `write_tx`
    /// clone drops and the writer task can exit, closing stdin (→ EOF).
    reader_task: Option<tokio::task::JoinHandle<()>>,
    log_path: PathBuf,
    pid_path: PathBuf,
    client: JsonRpcClient,
    responder: Arc<ReverseRpcResponder>,
}

impl AcpTestHarness {
    /// Spawn the loom-acp binary with `--home` pointed at `env`'s temp home.
    pub async fn spawn(env: &TestEnv, llm_url: &str) -> Self {
        let log_path = env.loom_home().join("loom-acp.log");
        let pid_path = env.loom_home().join("acp").join("loom-acp.pid");
        // `loom acp` is a stdio-to-WebSocket bridge.  Always use an isolated
        // ephemeral port so e2e tests cannot silently attach to a developer's
        // already-running server on the default 3030 endpoint.
        let port = TcpListener::bind(("127.0.0.1", 0))
            .expect("reserve an e2e ACP port")
            .local_addr()
            .expect("read reserved e2e ACP port")
            .port();

        let mut cmd = Command::new(binary_path());
        cmd.env("OPENAI_BASE_URL", format!("{llm_url}/v1"))
            .env("OPENAI_API_KEY", "test-key")
            .env("OPENAI_MODEL", "openai/gpt-4o")
            .env("LOOM_ACP_BRIDGE_EXIT_SHUTDOWN", "1")
            .arg("acp")
            .arg(format!("ws://127.0.0.1:{port}/acp"))
            .arg("--home")
            .arg(env.loom_home())
            .arg("--log-file")
            .arg(&log_path)
            .arg("--log-level")
            .arg("info")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .expect("failed to spawn loom-acp binary; check binary_path()");

        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        let stderr = child.stderr.take().expect("child stderr");

        // stderr drain for debugging
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("[loom-acp stderr] {line}");
            }
        });

        // Channel-based stdin writer
        let (write_tx, mut write_rx) = mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(frame) = write_rx.recv().await {
                let mut buf = frame.into_bytes();
                buf.push(b'\n');
                if stdin.write_all(&buf).await.is_err() {
                    break;
                }
                if stdin.flush().await.is_err() {
                    break;
                }
            }
        });

        let client = JsonRpcClient::new();
        let responder = Arc::new(ReverseRpcResponder::new());

        // Background reader
        let pending = client.pending_handle();
        let notifs = client.notifications_handle();
        let responder_for_reader = Arc::clone(&responder);
        let write_tx_for_reader = write_tx.clone();
        let reader_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(raw): Result<Value, _> = serde_json::from_str(&line) else {
                    continue;
                };
                let frame = JsonRpcFrame { raw: raw.clone() };
                if frame.is_response() {
                    if let Some(id) = frame.id() {
                        let mut map = pending.lock().await;
                        map.values.insert(id, raw.clone());
                        if let Some(notify) = map.map.remove(&id) {
                            notify.notify_one();
                        }
                    }
                } else if frame.is_request() {
                    // Reverse RPC: agent → harness
                    if let (Some(id), Some(method)) = (frame.id(), frame.method()) {
                        let result = match method {
                            "session/request_permission" => {
                                let params = &frame.raw["params"];
                                let tool_call_id = params["toolCallId"].as_str().unwrap_or("");
                                let tool_name = params["toolName"].as_str().unwrap_or("unknown");
                                responder_for_reader.answer_permission(tool_call_id, tool_name)
                            }
                            "fs/read_text_file" => {
                                let path = frame.raw["params"]["path"].as_str().unwrap_or("");
                                match responder_for_reader.read_file(std::path::Path::new(path)) {
                                    Some(content) => serde_json::json!({"content": content}),
                                    None => serde_json::json!({"content": ""}),
                                }
                            }
                            "fs/write_text_file" => {
                                serde_json::json!({})
                            }
                            "terminal/create" => {
                                serde_json::json!({"terminalId": "mock-term-1"})
                            }
                            "terminal/output" => {
                                serde_json::json!({})
                            }
                            "terminal/kill" => {
                                serde_json::json!({})
                            }
                            _ => serde_json::json!({}),
                        };
                        let response = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": result,
                        });
                        let _ = write_tx_for_reader.send(response.to_string());
                    }
                } else if frame.is_notification() {
                    if let Some(method) = frame.method() {
                        let params = frame.raw.get("params").cloned().unwrap_or(Value::Null);
                        notifs.lock().await.push(SessionNotification {
                            method: method.to_string(),
                            params,
                        });
                    }
                }
            }
        });

        Self {
            child,
            write_tx: Some(write_tx),
            reader_task: Some(reader_task),
            log_path,
            pid_path,
            client,
            responder,
        }
    }

    pub fn client(&self) -> &JsonRpcClient {
        &self.client
    }

    pub fn responder(&self) -> &Arc<ReverseRpcResponder> {
        &self.responder
    }

    pub fn log_path(&self) -> &std::path::Path {
        &self.log_path
    }

    pub fn pid_path(&self) -> &std::path::Path {
        &self.pid_path
    }

    // -----------------------------------------------------------------------
    // Frame writing
    // -----------------------------------------------------------------------

    /// Write a raw JSON string (already serialized) to stdin.
    fn send_raw(&self, frame: &str) {
        if let Some(tx) = &self.write_tx {
            tx.send(frame.to_string())
                .expect("stdin writer channel closed");
        }
    }

    /// Write a JSON-RPC frame to the child's stdin.
    pub async fn write_frame(&self, frame: &Value) {
        let line = serde_json::to_string(frame).expect("serialize frame");
        self.send_raw(&line);
    }

    // -----------------------------------------------------------------------
    // Typed request / notify helpers
    // -----------------------------------------------------------------------

    /// Send a JSON-RPC request and wait for the response.
    /// Returns the `result` field on success or panics on error/timeout.
    pub async fn request(&self, method: &str, params: Value) -> Value {
        let (id, frame) = self.client.build_request(method, params);
        let notify = self.client.register_pending(id).await;
        self.write_frame(&frame).await;
        match tokio::time::timeout(DEFAULT_REQUEST_TIMEOUT, notify.notified()).await {
            Ok(()) => {}
            Err(_) => {
                self.dump_log_tail().await;
                panic!("request '{method}' (id={id}) timed out after {DEFAULT_REQUEST_TIMEOUT:?}");
            }
        }
        let response = self
            .client
            .take_response(id)
            .await
            .unwrap_or_else(|| panic!("response for id={id} missing after notify"));
        if let Some(error) = response.get("error") {
            panic!("request '{method}' (id={id}) returned error: {error}");
        }
        response.get("result").cloned().unwrap_or(Value::Null)
    }

    /// Like `request` but returns the raw response Value (including errors).
    pub async fn request_raw(&self, method: &str, params: Value) -> Value {
        let (id, frame) = self.client.build_request(method, params);
        let notify = self.client.register_pending(id).await;
        self.write_frame(&frame).await;
        match tokio::time::timeout(DEFAULT_REQUEST_TIMEOUT, notify.notified()).await {
            Ok(()) => {}
            Err(_) => {
                self.dump_log_tail().await;
                panic!("request '{method}' (id={id}) timed out");
            }
        }
        self.client
            .take_response(id)
            .await
            .unwrap_or_else(|| panic!("response for id={id} missing"))
    }

    /// Send a JSON-RPC notification (no response expected).
    pub async fn notify(&self, method: &str, params: Value) {
        let frame = self.client.build_notification(method, params);
        self.write_frame(&frame).await;
    }

    // -----------------------------------------------------------------------
    // Notification helpers
    // -----------------------------------------------------------------------

    /// Drain all buffered notifications.
    pub async fn drain_notifications(&self) -> Vec<SessionNotification> {
        self.client.drain_notifications().await
    }

    /// Drain and return notifications matching `pred`, keeping the rest.
    pub async fn drain_matching<F>(&self, pred: F) -> Vec<SessionNotification>
    where
        F: Fn(&SessionNotification) -> bool,
    {
        self.client.drain_matching(pred).await
    }

    /// Wait until at least one notification matches `pred`, draining all
    /// notifications in the meantime. Returns matching notifications.
    pub async fn wait_for_notification<F>(
        &self,
        pred: F,
        timeout: Duration,
    ) -> Vec<SessionNotification>
    where
        F: Fn(&SessionNotification) -> bool + Send + 'static,
    {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            // Check current buffer
            let matching = self.drain_matching(&pred).await;
            if !matching.is_empty() {
                return matching;
            }
            if tokio::time::Instant::now() >= deadline {
                self.dump_log_tail().await;
                let all = self.drain_notifications().await;
                panic!(
                    "wait_for_notification timed out after {timeout:?}.\n\
                     Buffered notifications ({}): {:#?}",
                    all.len(),
                    all
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    // -----------------------------------------------------------------------
    // Shutdown
    // -----------------------------------------------------------------------

    /// Dump the last N lines of the child's log file to stderr (debugging aid).
    pub async fn dump_log_tail(&self) {
        if let Ok(content) = tokio::fs::read_to_string(&self.log_path).await {
            let lines: Vec<&str> = content.lines().collect();
            let tail: Vec<&str> = if lines.len() > 60 {
                lines[lines.len() - 60..].to_vec()
            } else {
                lines
            };
            eprintln!("=== loom-acp log tail (last {} lines) ===", tail.len());
            for line in tail {
                eprintln!("  {line}");
            }
        } else {
            eprintln!("=== (no log file at {}) ===", self.log_path.display());
        }
    }

    /// Graceful shutdown: close stdin (EOF), wait for child to exit cleanly.
    pub async fn shutdown(mut self) -> std::process::ExitStatus {
        // 1. Drop our write_tx — but the reader task still has a clone.
        self.write_tx.take();
        // 2. Abort the reader task so its write_tx clone drops too.
        if let Some(handle) = self.reader_task.take() {
            handle.abort();
            let _ = handle.await;
        }
        // Now all senders are dropped → writer task exits → ChildStdin drops →
        // stdin pipe closes → binary sees EOF → exits cleanly.
        match tokio::time::timeout(GRACEFUL_EXIT_TIMEOUT, self.child.wait()).await {
            Ok(Ok(status)) => status,
            Ok(Err(e)) => panic!("wait child failed: {e}"),
            Err(_) => {
                let _ = self.child.start_kill();
                panic!("loom-acp did not exit within {GRACEFUL_EXIT_TIMEOUT:?}");
            }
        }
    }
}

impl Drop for AcpTestHarness {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}
