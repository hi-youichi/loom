use loom_acp::terminal::{TerminalManager, TerminalStatus};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TerminalCall {
    pub method: String,
    pub params: Value,
    pub response_result: Option<Value>,
}

#[allow(dead_code)]
pub struct TerminalHandler {
    #[allow(dead_code)]
    runtime: std::thread::JoinHandle<()>,
    #[allow(dead_code)]
    manager: Arc<TerminalManager>,
    sender: std::sync::mpsc::Sender<PendingRequest>,
    calls: Arc<std::sync::Mutex<Vec<TerminalCall>>>,
}

struct PendingRequest {
    method: String,
    id: Value,
    params: Value,
    tx: std::sync::mpsc::Sender<Value>,
}

macro_rules! tlog {
    ($($arg:tt)*) => {
        eprintln!("[terminal-handler] {}", format!($($arg)*))
    };
}

impl TerminalHandler {
    pub fn new() -> Self {
        tlog!("creating TerminalHandler with dedicated OS thread");
        let manager = Arc::new(TerminalManager::new());
        let (req_tx, req_rx) = std::sync::mpsc::channel::<PendingRequest>();

        let mgr = manager.clone();
        let handle = std::thread::spawn(move || {
            tlog!("handler thread started");
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("create tokio runtime");
            tlog!("handler thread tokio runtime ready");
            while let Ok(req) = req_rx.recv() {
                tlog!("received request: method={} id={}", req.method, req.id);
                let started = std::time::Instant::now();
                let resp = match req.method.as_str() {
                    "terminal/create" => {
                        let mgr = mgr.clone();
                        rt.block_on(async {
                            Self::handle_create_async(&mgr, &req.id, &req.params).await
                        })
                    }
                    "terminal/output" => {
                        let mgr = mgr.clone();
                        rt.block_on(async {
                            Self::handle_output_async(&mgr, &req.id, &req.params).await
                        })
                    }
                    "terminal/wait_for_exit" => {
                        let mgr = mgr.clone();
                        rt.block_on(async {
                            Self::handle_wait_for_exit_async(&mgr, &req.id, &req.params).await
                        })
                    }
                    "terminal/kill" => {
                        let mgr = mgr.clone();
                        rt.block_on(async {
                            Self::handle_kill_async(&mgr, &req.id, &req.params).await
                        })
                    }
                    "terminal/release" => {
                        let mgr = mgr.clone();
                        rt.block_on(async {
                            Self::handle_release_async(&mgr, &req.id, &req.params).await
                        })
                    }
                    _ => {
                        tlog!("unknown method: {}", req.method);
                        json!({
                            "jsonrpc": "2.0",
                            "id": req.id,
                            "error": { "code": -32601, "message": format!("unknown method: {}", req.method) }
                        })
                    }
                };
                let elapsed = started.elapsed();
                let is_error = resp.get("error").is_some();
                if is_error {
                    tlog!("request FAILED: method={} id={} elapsed={:?} error={}", req.method, req.id, elapsed, resp.get("error").unwrap());
                } else {
                    let result_summary = Self::summarize_result(&req.method, &resp);
                    tlog!("request OK: method={} id={} elapsed={} result={}", req.method, req.id, elapsed.as_millis(), result_summary);
                }
                let _ = req.tx.send(resp);
            }
            tlog!("handler thread exiting (channel closed)");
        });

        Self {
            runtime: handle,
            manager,
            sender: req_tx,
            calls: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    fn summarize_result(method: &str, resp: &Value) -> String {
        let result = match resp.get("result") {
            Some(r) => r,
            None => return "none".to_string(),
        };
        match method {
            "terminal/create" => {
                let tid = result.get("terminalId").and_then(|v| v.as_str()).unwrap_or("?");
                format!("terminalId={}", tid)
            }
            "terminal/output" => {
                let output = result.get("output").and_then(|v| v.as_str()).unwrap_or("");
                let truncated = result.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false);
                let exit_code = result.get("exitCode");
                let preview = if output.len() > 80 {
                    format!("{}...(truncated to 80 chars, total={})", &output[..80], output.len())
                } else {
                    output.to_string()
                };
                format!("output={:?} truncated={} exit_code={:?}", preview, truncated, exit_code)
            }
            "terminal/wait_for_exit" => {
                let exit_code = result.get("exitCode");
                let signal = result.get("signal").and_then(|v| v.as_str());
                format!("exit_code={:?} signal={:?}", exit_code, signal)
            }
            "terminal/kill" => "ok".to_string(),
            "terminal/release" => "ok".to_string(),
            _ => result.to_string(),
        }
    }

    pub fn handle_request(&self, method: &str, id: &Value, params: &Value) -> Option<Value> {
        tlog!("handle_request: method={} id={}", method, id);
        let (tx, rx) = std::sync::mpsc::channel::<Value>();
        let req = PendingRequest {
            method: method.to_string(),
            id: id.clone(),
            params: params.clone(),
            tx,
        };
        if self.sender.send(req).is_err() {
            tlog!("handle_request: handler thread gone!");
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32603, "message": "terminal handler thread gone" }
            }));
        }
        let response = rx.recv().ok()?;
        self.calls.lock().unwrap().push(TerminalCall {
            method: method.to_string(),
            params: params.clone(),
            response_result: response.get("result").cloned(),
        });
        tlog!("handle_request done: method={} total_calls={}", method, self.calls.lock().unwrap().len());
        Some(response)
    }

    pub fn take_calls(&self) -> Vec<TerminalCall> {
        self.calls.lock().unwrap().drain(..).collect()
    }

    async fn handle_create_async(manager: &TerminalManager, id: &Value, params: &Value) -> Value {
        let command = params.get("command").and_then(|v| v.as_str()).unwrap_or("");
        let args = params
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let cwd = params
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(|s| std::path::PathBuf::from(s));
        let env = params
            .get("env")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        let name = v.get("name")?.as_str()?.to_string();
                        let value = v.get("value")?.as_str()?.to_string();
                        Some((name, value))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        tlog!(
            "create: command={} args={:?} cwd={:?} env_count={}",
            command, args, cwd, env.len()
        );

        match manager
            .create_terminal(command.to_string(), args, cwd, env, None)
            .await
        {
            Ok(terminal_id) => {
                tlog!("create: success terminalId={}", terminal_id);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "terminalId": terminal_id }
                })
            }
            Err(e) => {
                tlog!("create: FAILED error={}", e);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32603, "message": e.to_string() }
                })
            }
        }
    }

    async fn handle_output_async(manager: &TerminalManager, id: &Value, params: &Value) -> Value {
        let terminal_id = params
            .get("terminalId")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        tlog!("output: terminalId={}", terminal_id);

        match manager.get_output(terminal_id).await {
            Some((output, truncated, status)) => {
                let exit_code = match &status {
                    Some(TerminalStatus::Completed { exit_code, .. }) => *exit_code,
                    _ => None,
                };
                tlog!(
                    "output: terminalId={} bytes={} truncated={} exit_code={:?}",
                    terminal_id, output.len(), truncated, exit_code
                );
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "output": output,
                        "truncated": truncated,
                        "exitCode": exit_code
                    }
                })
            }
            None => {
                tlog!("output: NOT FOUND terminalId={}", terminal_id);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32602, "message": format!("terminal not found: {}", terminal_id) }
                })
            }
        }
    }

    async fn handle_wait_for_exit_async(
        manager: &TerminalManager,
        id: &Value,
        params: &Value,
    ) -> Value {
        let terminal_id = params
            .get("terminalId")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        tlog!("wait_for_exit: terminalId={}", terminal_id);

        match manager.wait_for_exit(terminal_id).await {
            Ok(status) => {
                let (exit_code, signal) = match &status {
                    TerminalStatus::Completed { exit_code, signal } => (*exit_code, signal.clone()),
                    TerminalStatus::Killed => (None, Some("SIGKILL".to_string())),
                    _ => (None, None),
                };
                tlog!(
                    "wait_for_exit: terminalId={} exit_code={:?} signal={:?}",
                    terminal_id, exit_code, signal
                );
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "exitCode": exit_code,
                        "signal": signal
                    }
                })
            }
            Err(e) => {
                tlog!("wait_for_exit: FAILED terminalId={} error={}", terminal_id, e);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32602, "message": e.to_string() }
                })
            }
        }
    }

    async fn handle_kill_async(manager: &TerminalManager, id: &Value, params: &Value) -> Value {
        let terminal_id = params
            .get("terminalId")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        tlog!("kill: terminalId={}", terminal_id);

        match manager.kill(terminal_id).await {
            Ok(()) => {
                tlog!("kill: OK terminalId={}", terminal_id);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {}
                })
            }
            Err(e) => {
                tlog!("kill: FAILED terminalId={} error={}", terminal_id, e);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32602, "message": e.to_string() }
                })
            }
        }
    }

    async fn handle_release_async(manager: &TerminalManager, id: &Value, params: &Value) -> Value {
        let terminal_id = params
            .get("terminalId")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        tlog!("release: terminalId={}", terminal_id);

        match manager.release(terminal_id).await {
            Ok(()) => {
                tlog!("release: OK terminalId={}", terminal_id);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {}
                })
            }
            Err(e) => {
                tlog!("release: FAILED terminalId={} error={}", terminal_id, e);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32602, "message": e.to_string() }
                })
            }
        }
    }
}

pub fn is_terminal_method(method: &str) -> bool {
    matches!(
        method,
        "terminal/create"
            | "terminal/output"
            | "terminal/wait_for_exit"
            | "terminal/kill"
            | "terminal/release"
    )
}
