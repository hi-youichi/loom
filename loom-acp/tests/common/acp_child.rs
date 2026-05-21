use super::terminal_handler::{is_terminal_method, TerminalCall, TerminalHandler};
use super::RpcResponse;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Simple mock server for testing
pub struct MockAcpServer {
    pub server: wiremock::MockServer,
}

impl MockAcpServer {
    pub async fn start() -> Self {
        let server = wiremock::MockServer::start().await;
        Self { server }
    }

    pub async fn mount_default_responses(&self) {
        use wiremock::matchers::{method, path};
        use wiremock::Respond;
        use wiremock::{Mock, ResponseTemplate};

        struct CompletionResponder;
        impl Respond for CompletionResponder {
            fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
                let is_stream = std::str::from_utf8(&request.body)
                    .map(|s| s.contains("\"stream\":true") || s.contains("\"stream\": true"))
                    .unwrap_or(false);
                if is_stream {
                    ResponseTemplate::new(200)
                        .set_body_raw(Self::streaming_body().into_bytes(), "text/event-stream")
                } else {
                    ResponseTemplate::new(200).set_body_json(Self::simple_completion())
                }
            }
        }

        impl CompletionResponder {
            fn simple_completion() -> serde_json::Value {
                json!({
                    "id": "chatcmpl-mock",
                    "object": "chat.completion",
                    "created": 1234567890,
                    "model": "test-model",
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "Done."
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": { "prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7 }
                })
            }

            fn streaming_body() -> String {
                let chunk = json!({
                    "id": "chatcmpl-mock",
                    "object": "chat.completion.chunk",
                    "created": 1234567890,
                    "model": "test-model",
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "role": "assistant",
                            "content": "Done."
                        },
                        "finish_reason": null
                    }]
                });
                let done_chunk = json!({
                    "id": "chatcmpl-mock",
                    "object": "chat.completion.chunk",
                    "created": 1234567890,
                    "model": "test-model",
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": "stop"
                    }]
                });
                format!(
                    "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                    chunk, done_chunk
                )
            }
        }

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(CompletionResponder)
            .mount(&self.server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(Self::models_list()))
            .mount(&self.server)
            .await;
    }

    #[allow(dead_code)]
    pub async fn mount_completion(&self, response_body: serde_json::Value) {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&self.server)
            .await;
    }

    fn models_list() -> serde_json::Value {
        json!({
            "object": "list",
            "data": [{
                "id": "test-model",
                "object": "model",
                "created": 1234567890,
                "owned_by": "mock"
            }]
        })
    }

    #[allow(dead_code)]
    pub async fn mount_tool_call_response(&self, _responses: &[ToolCallResponse]) {
        self.mount_default_responses().await;
    }

    #[allow(dead_code)]
    pub async fn mount_bash_tool_call(
        &self,
        command: &str,
    ) -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
        self.mount_bash_tool_call_with_timeout(command, None).await
    }

    #[allow(dead_code)]
    pub async fn mount_bash_tool_call_with_timeout(
        &self,
        command: &str,
        timeout_ms: Option<u64>,
    ) -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use wiremock::matchers::{method, path};
        use wiremock::Respond;
        use wiremock::{Mock, ResponseTemplate};

        self.server.reset().await;

        let counter = std::sync::Arc::new(AtomicUsize::new(0));
        let command = command.to_string();
        let counter_clone = counter.clone();

        struct BashToolCallResponder {
            call_count: std::sync::Arc<AtomicUsize>,
            command: String,
            timeout_ms: Option<u64>,
        }

        impl Respond for BashToolCallResponder {
            fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
                let is_stream = std::str::from_utf8(&request.body)
                    .map(|s| s.contains("\"stream\":true") || s.contains("\"stream\": true"))
                    .unwrap_or(false);
                let count = self.call_count.fetch_add(1, Ordering::SeqCst);
                let mut args = json!({"command": self.command});
                if let Some(timeout) = self.timeout_ms {
                    args["timeout_ms"] = json!(timeout);
                }
                let args_str = serde_json::to_string(&args).unwrap();
                if count == 0 {
                    if is_stream {
                        let chunk = json!({
                            "id": "chatcmpl-mock",
                            "object": "chat.completion.chunk",
                            "created": 1234567890,
                            "model": "test-model",
                            "choices": [{
                                "index": 0,
                                "delta": {
                                    "role": "assistant",
                                    "tool_calls": [{
                                        "index": 0,
                                        "id": "call_test_1",
                                        "type": "function",
                                        "function": {
                                            "name": "bash",
                                            "arguments": args_str
                                        }
                                    }]
                                },
                                "finish_reason": null
                            }]
                        });
                        let done_chunk = json!({
                            "id": "chatcmpl-mock",
                            "object": "chat.completion.chunk",
                            "created": 1234567890,
                            "model": "test-model",
                            "choices": [{
                                "index": 0,
                                "delta": {},
                                "finish_reason": "tool_calls"
                            }]
                        });
                        let sse = format!(
                            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                            chunk, done_chunk
                        );
                        ResponseTemplate::new(200)
                            .set_body_raw(sse.into_bytes(), "text/event-stream")
                    } else {
                        ResponseTemplate::new(200).set_body_json(json!({
                            "id": "chatcmpl-mock",
                            "object": "chat.completion",
                            "created": 1234567890,
                            "model": "test-model",
                            "choices": [{
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": null,
                                    "tool_calls": [{
                                        "id": "call_test_1",
                                        "type": "function",
                                        "function": {
                                            "name": "bash",
                                            "arguments": args_str
                                        }
                                    }]
                                },
                                "finish_reason": "tool_calls"
                            }],
                            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
                        }))
                    }
                } else {
                    if is_stream {
                        let chunk = json!({
                            "id": "chatcmpl-mock",
                            "object": "chat.completion.chunk",
                            "created": 1234567890,
                            "model": "test-model",
                            "choices": [{
                                "index": 0,
                                "delta": { "role": "assistant", "content": "Done." },
                                "finish_reason": null
                            }]
                        });
                        let done_chunk = json!({
                            "id": "chatcmpl-mock",
                            "object": "chat.completion.chunk",
                            "created": 1234567890,
                            "model": "test-model",
                            "choices": [{
                                "index": 0,
                                "delta": {},
                                "finish_reason": "stop"
                            }]
                        });
                        let sse = format!(
                            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                            chunk, done_chunk
                        );
                        ResponseTemplate::new(200)
                            .set_body_raw(sse.into_bytes(), "text/event-stream")
                    } else {
                        ResponseTemplate::new(200).set_body_json(json!({
                            "id": "chatcmpl-mock",
                            "object": "chat.completion",
                            "created": 1234567890,
                            "model": "test-model",
                            "choices": [{
                                "index": 0,
                                "message": { "role": "assistant", "content": "Done." },
                                "finish_reason": "stop"
                            }],
                            "usage": { "prompt_tokens": 20, "completion_tokens": 2, "total_tokens": 22 }
                        }))
                    }
                }
            }
        }

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(BashToolCallResponder {
                call_count: counter_clone,
                command,
                timeout_ms,
            })
            .mount(&self.server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(Self::models_list()))
            .mount(&self.server)
            .await;

        counter
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallResponse {
    #[allow(dead_code)]
    pub tool_name: String,
    #[allow(dead_code)]
    pub parameters: serde_json::Value,
}

pub struct AcpChild {
    process: Child,
    pub reader: BufReader<std::process::ChildStdout>,
    pub writer: Arc<Mutex<std::process::ChildStdin>>,
    request_id: Arc<Mutex<u64>>,
    _temp_dir: Option<tempfile::TempDir>,
    terminal_handler: Option<TerminalHandler>,
}

#[allow(dead_code)]
impl AcpChild {
    pub fn spawn(home: Option<&Path>) -> Result<Self, Box<dyn std::error::Error>> {
        Self::spawn_with_temp_dir(home, None)
    }

    pub fn spawn_with_temp_dir(
        home: Option<&Path>,
        temp_dir: Option<tempfile::TempDir>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let actual_home = if let Some(h) = home {
            h.to_path_buf()
        } else {
            std::env::current_dir().expect("Failed to get current directory")
        };

        let bin = env!("CARGO_BIN_EXE_loom-acp");

        let log_dir =
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/logs"));
        let _ = std::fs::create_dir_all(&log_dir);
        let log_file = log_dir.join("loom-acp-e2e.log");

        let mut process = Command::new(bin)
            .arg("--log-level")
            .arg("trace")
            .arg("--log-file")
            .arg(&log_file)
            .env("LOOM_HOME", &actual_home)
            .env("OPENAI_API_KEY", "test-key")
            .env("LOOM_GOAL_MAX_ITERATIONS", "1")
            .env_remove("OPENAI_BASE_URL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = process.stdout.take().ok_or("Failed to capture stdout")?;
        let stdin = process.stdin.take().ok_or("Failed to capture stdin")?;

        let reader = BufReader::new(stdout);
        let writer = Arc::new(Mutex::new(stdin));

        Ok(Self {
            process,
            reader,
            writer,
            request_id: Arc::new(Mutex::new(0)),
            _temp_dir: temp_dir,
            terminal_handler: None,
        })
    }

    pub async fn spawn_with_mock() -> Result<(Self, MockAcpServer), Box<dyn std::error::Error>> {
        Self::spawn_with_mock_and_capabilities(false).await
    }

    pub async fn spawn_with_mock_at_home(
        home: &Path,
    ) -> Result<(Self, MockAcpServer), Box<dyn std::error::Error>> {
        let mock = MockAcpServer::start().await;

        let config_toml = format!(
            r#"[default]
provider = "mock"

[[providers]]
name = "mock"
api_key = "test-key"
base_url = "{}/v1"
model = "test-model"
"#,
            mock.server.uri()
        );
        std::fs::write(home.join("config.toml"), config_toml)?;

        let acp = Self::spawn_with_temp_dir(Some(home), None)?;
        mock.mount_default_responses().await;

        Ok((acp, mock))
    }

    pub async fn spawn_with_mock_and_terminal(
    ) -> Result<(Self, MockAcpServer), Box<dyn std::error::Error>> {
        Self::spawn_with_mock_and_capabilities(true).await
    }

    async fn spawn_with_mock_and_capabilities(
        terminal: bool,
    ) -> Result<(Self, MockAcpServer), Box<dyn std::error::Error>> {
        let mock = MockAcpServer::start().await;
        let temp_dir = tempfile::tempdir()?;
        let home = temp_dir.path().to_path_buf();

        let config_toml = format!(
            r#"[default]
provider = "mock"

[[providers]]
name = "mock"
api_key = "test-key"
base_url = "{}/v1"
model = "test-model"
"#,
            mock.server.uri()
        );
        std::fs::write(home.join("config.toml"), config_toml)?;

        let mut acp = Self::spawn_with_temp_dir(Some(&home), Some(temp_dir))?;
        if terminal {
            acp.terminal_handler = Some(TerminalHandler::new());
        }
        mock.mount_default_responses().await;

        Ok((acp, mock))
    }

    pub fn next_request_id(&self) -> u64 {
        let mut id = self.request_id.lock().unwrap();
        *id += 1;
        *id
    }

    pub fn current_request_id(&self) -> u64 {
        let id = self.request_id.lock().unwrap();
        *id
    }

    pub async fn call(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let request_id = self.next_request_id();
        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params
        });

        let request_str = serde_json::to_string(&request)?;

        {
            let mut writer = self.writer.lock().unwrap();
            writeln!(writer, "{}", request_str)?;
            writer.flush()?;
        }

        // Read response
        let mut line = String::new();
        self.reader.read_line(&mut line)?;

        let response: Value = serde_json::from_str(&line)?;

        if let Some(error) = response.get("error") {
            return Err(format!("RPC error: {}", error).into());
        }

        Ok(response.get("result").cloned().unwrap_or(json!(null)))
    }

    pub async fn initialize_session(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.call(
            "initialize",
            json!({
                "protocolVersion": "0.10",
                "capabilities": {},
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                }
            }),
        )
        .await?;

        self.call("initialized", json!({})).await?;

        Ok(())
    }

    pub async fn set_model_option(
        &mut self,
        model: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.call(
            "session/set_config_option",
            json!({
                "key": "model",
                "value": model
            }),
        )
        .await?;

        Ok(())
    }

    pub async fn send_request(
        &mut self,
        content: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let result = self
            .call(
                "textDocument/complete",
                json!({
                    "text": content
                }),
            )
            .await?;

        Ok(result.to_string())
    }

    pub async fn invoke_subagent(
        &mut self,
        agent_name: &str,
        task: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let result = self
            .call(
                "agent/invoke",
                json!({
                    "agent": agent_name,
                    "task": task
                }),
            )
            .await?;

        Ok(result.to_string())
    }

    pub fn wait_for_exit(&mut self, _timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.process.try_wait()?;
        // In a real implementation, we'd wait with timeout
        Ok(())
    }

    pub fn is_alive(&mut self) -> bool {
        self.process.try_wait()
            .map(|status| status.is_none())
            .unwrap_or(false)
    }

    pub fn has_terminal_handler(&self) -> bool {
        self.terminal_handler.is_some()
    }

    pub fn take_terminal_calls(&self) -> Vec<TerminalCall> {
        self.terminal_handler
            .as_ref()
            .map(|h| h.take_calls())
            .unwrap_or_default()
    }

    pub async fn handshake_with_capabilities(
        &mut self,
        capabilities: Value,
    ) -> Result<String, Box<dyn std::error::Error>> {
        eprintln!("[acp-child] handshake_with_capabilities: caps={}", capabilities);
        self.call(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "clientCapabilities": capabilities,
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                }
            }),
        )
        .await?;

        let session_result = self
            .call(
                "session/new",
                json!({
                    "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
                    "mcpServers": [],
                }),
            )
            .await?;

        let session_id = session_result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .ok_or("No sessionId in response")?
            .to_string();

        eprintln!("[acp-child] handshake complete: session_id={}", session_id);
        Ok(session_id)
    }

    pub async fn handshake(
        &mut self,
        _timeout: Duration,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let caps = if self.terminal_handler.is_some() {
            json!({ "terminal": true })
        } else {
            json!({})
        };
        self.handshake_with_capabilities(caps).await
    }

    fn handle_agent_request(&mut self, msg: &Value) -> Result<(), Box<dyn std::error::Error>> {
        let handler = match self.terminal_handler {
            Some(ref h) => h,
            None => return Err("no terminal handler".into()),
        };
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        if !is_terminal_method(method) {
            return Err(format!("unhandled agent request method: {}", method).into());
        }
        let id = msg.get("id").cloned().unwrap_or(json!(null));
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        eprintln!("[acp-child] >>> agent request: method={} id={}", method, id);
        if let Some(response) = handler.handle_request(method, &id, &params) {
            let is_error = response.get("error").is_some();
            eprintln!("[acp-child] <<< agent response: method={} id={} is_error={}", method, id, is_error);
            let response_str = serde_json::to_string(&response)?;
            let mut writer = self.writer.lock().unwrap();
            writeln!(writer, "{}", response_str)?;
            writer.flush()?;
        }
        Ok(())
    }

    pub fn collect_all_notifications_handling_terminal(
        &mut self,
        request_id: u64,
        timeout: Duration,
    ) -> Result<(Vec<serde_json::Value>, RpcResponse), Box<dyn std::error::Error>> {
        let mut notifications = Vec::new();
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() > timeout {
                return Err("timeout waiting for response".into());
            }
            let mut line = String::new();
            let bytes = self.reader.read_line(&mut line)?;
            if bytes == 0 {
                return Err("EOF while reading response".into());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let msg: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[acp-child] parse error: {} line={:?}", e, trimmed.get(..80).unwrap_or(""));
                    continue;
                }
            };

            let has_id = msg.get("id").is_some();
            let has_method = msg.get("method").is_some();

            if has_id && !has_method {
                let id_val = msg.get("id").and_then(|v| v.as_u64());
                eprintln!("[acp-child] response: id={:?} looking_for={}", id_val, request_id);
                if id_val == Some(request_id) {
                    let response: RpcResponse = serde_json::from_value(msg)?;
                    eprintln!("[acp-child] matched response for request_id={}, notifications={}", request_id, notifications.len());
                    return Ok((notifications, response));
                }
                continue;
            }

            if has_id && has_method {
                let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
                eprintln!("[acp-child] agent->client request: method={} id={:?}", method, msg.get("id"));
                if is_terminal_method(method) {
                    self.handle_agent_request(&msg)?;
                    continue;
                }
            }

            if has_method {
                let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
                let update_type = msg.get("params")
                    .and_then(|p| p.get("update"))
                    .and_then(|u| u.get("sessionUpdate"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                eprintln!("[acp-child] notification: method={} sessionUpdate={}", method, update_type);
                notifications.push(msg);
            }
        }
    }

    pub fn read_message(&mut self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        Ok(serde_json::from_str(&line)?)
    }

    pub fn collect_all_notifications_with_drain(
        &mut self,
        request_id: u64,
        timeout: Duration,
        _post_response_drain: Duration,
    ) -> Result<(Vec<serde_json::Value>, RpcResponse), Box<dyn std::error::Error>> {
        self.collect_all_notifications(request_id, timeout)
    }

    pub async fn send_request_and_wait(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<RpcResponse, Box<dyn std::error::Error>> {
        let request_id = self.next_request_id();
        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params
        });

        let request_str = serde_json::to_string(&request)?;

        {
            let mut writer = self.writer.lock().unwrap();
            writeln!(writer, "{}", request_str)?;
            writer.flush()?;
        }

        // Read response with timeout
        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > timeout {
                return Err("Request timeout".into());
            }

            let mut line = String::new();
            self.reader.read_line(&mut line)?;

            if let Ok(response) = serde_json::from_str::<RpcResponse>(&line) {
                if response.id.as_ref().and_then(|v| v.as_u64()) == Some(request_id) {
                    return Ok(response);
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn wait(&mut self) -> Result<std::process::ExitStatus, Box<dyn std::error::Error>> {
        Ok(self.process.wait()?)
    }

    #[allow(dead_code)]
    pub fn drop_stdin(&mut self) {
        // This method is used to signal EOF to the process
        // We don't actually drop stdin since it's managed by Arc<Mutex>
        // In a real implementation, we might close the write end
    }

    pub fn prompt_and_collect_plans(
        &mut self,
        session_id: &str,
        text: &str,
        timeout: Duration,
    ) -> Result<(Vec<super::plan_types::PlanNotification>, RpcResponse), Box<dyn std::error::Error>>
    {
        let request_id = self.next_request_id();
        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "session/prompt",
            "params": {
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }]
            }
        });

        {
            let mut writer = self.writer.lock().unwrap();
            writeln!(writer, "{}", serde_json::to_string(&request)?)?;
            writer.flush()?;
        }

        let mut plans = Vec::new();
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() > timeout {
                return Err("timeout waiting for prompt response".into());
            }
            let mut line = String::new();
            let bytes = self.reader.read_line(&mut line)?;
            if bytes == 0 {
                return Err("EOF while reading response".into());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let msg: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if msg.get("id").and_then(|v| v.as_u64()) == Some(request_id) {
                let response: RpcResponse = serde_json::from_value(msg)?;
                return Ok((plans, response));
            }

            if msg.get("method").and_then(|v| v.as_str()) == Some("session/update") {
                if let Some(update) = msg.get("params").and_then(|p| p.get("update")) {
                    if update.get("sessionUpdate").and_then(|v| v.as_str()) == Some("plan") {
                        if let Ok(plan_notif) = serde_json::from_value::<
                            super::plan_types::PlanNotification,
                        >(update.clone())
                        {
                            plans.push(plan_notif);
                        }
                    }
                }
            }
        }
    }

    pub fn collect_all_notifications(
        &mut self,
        request_id: u64,
        timeout: Duration,
    ) -> Result<(Vec<serde_json::Value>, RpcResponse), Box<dyn std::error::Error>> {
        let mut notifications = Vec::new();
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() > timeout {
                return Err("timeout waiting for response".into());
            }
            let mut line = String::new();
            let bytes = self.reader.read_line(&mut line)?;
            if bytes == 0 {
                return Err("EOF while reading response".into());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let msg: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if msg.get("id").and_then(|v| v.as_u64()) == Some(request_id) {
                let response: RpcResponse = serde_json::from_value(msg)?;
                return Ok((notifications, response));
            }

            if msg.get("method").is_some() && msg.get("id").is_none() {
                notifications.push(msg);
            }
        }
    }

    pub fn send_prompt_request(
        &mut self,
        session_id: &str,
        text: &str,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let request_id = self.next_request_id();
        eprintln!("[acp-child] send_prompt_request: session_id={} request_id={} text={:?}", session_id, request_id, text.get(..80).unwrap_or(text));
        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "session/prompt",
            "params": {
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }]
            }
        });

        {
            let mut writer = self.writer.lock().unwrap();
            writeln!(writer, "{}", serde_json::to_string(&request)?)?;
            writer.flush()?;
        }
        Ok(request_id)
    }

    pub fn prompt_and_collect_with_terminal(
        &mut self,
        session_id: &str,
        text: &str,
        timeout: Duration,
    ) -> Result<(Vec<serde_json::Value>, RpcResponse), Box<dyn std::error::Error>> {
        let request_id = self.next_request_id();
        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "session/prompt",
            "params": {
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }]
            }
        });

        {
            let mut writer = self.writer.lock().unwrap();
            writeln!(writer, "{}", serde_json::to_string(&request)?)?;
            writer.flush()?;
        }

        self.collect_all_notifications_handling_terminal(request_id, timeout)
    }

    pub fn send_raw(&mut self, raw: &str) -> Result<(), Box<dyn std::error::Error>> {
        {
            let mut writer = self.writer.lock().unwrap();
            writeln!(writer, "{}", raw)?;
            writer.flush()?;
        }
        Ok(())
    }

    pub fn send_load_request(
        &mut self,
        session_id: &str,
        cwd: &str,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let request_id = self.next_request_id();
        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "session/load",
            "params": {
                "sessionId": session_id,
                "cwd": cwd,
                "mcpServers": []
            }
        });
        let mut writer = self.writer.lock().unwrap();
        writeln!(writer, "{}", serde_json::to_string(&request)?)?;
        writer.flush()?;
        Ok(request_id)
    }

    pub fn load_and_collect_notifications(
        &mut self,
        session_id: &str,
        cwd: &str,
        timeout: Duration,
    ) -> Result<(Vec<serde_json::Value>, RpcResponse), Box<dyn std::error::Error>> {
        let request_id = self.send_load_request(session_id, cwd)?;
        self.collect_all_notifications(request_id, timeout)
    }
}

impl Drop for AcpChild {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}
