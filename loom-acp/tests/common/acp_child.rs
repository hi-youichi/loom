use std::process::{Child, Stdio, Command};
use std::path::Path;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use super::RpcResponse;

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
        use wiremock::{Mock, ResponseTemplate};
        use wiremock::matchers::{method, path};

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(Self::simple_completion()))
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
        use wiremock::{Mock, ResponseTemplate};
        use wiremock::matchers::{method, path};

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&self.server)
            .await;
    }

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
}

#[allow(dead_code)]
impl AcpChild {
    pub fn spawn(home: Option<&Path>) -> Result<Self, Box<dyn std::error::Error>> {
        Self::spawn_with_temp_dir(home, None)
    }

    pub fn spawn_with_temp_dir(home: Option<&Path>, temp_dir: Option<tempfile::TempDir>) -> Result<Self, Box<dyn std::error::Error>> {
        let actual_home = if let Some(h) = home {
            h.to_path_buf()
        } else {
            std::env::current_dir().expect("Failed to get current directory")
        };
        
        let bin = env!("CARGO_BIN_EXE_loom-acp");
        let mut process = Command::new(bin)
            .env("LOOM_HOME", &actual_home)
            .env("OPENAI_API_KEY", "test-key")
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
        })
    }
    
    pub async fn spawn_with_mock() -> Result<(Self, MockAcpServer), Box<dyn std::error::Error>> {
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

        let acp = Self::spawn_with_temp_dir(Some(&home), Some(temp_dir))?;
        mock.mount_default_responses().await;

        Ok((acp, mock))
    }
    
    fn next_request_id(&self) -> u64 {
        let mut id = self.request_id.lock().unwrap();
        *id += 1;
        *id
    }
    
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value, Box<dyn std::error::Error>> {
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
        self.call("initialize", json!({
            "protocolVersion": "0.10",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        })).await?;
        
        self.call("initialized", json!({})).await?;
        
        Ok(())
    }
    
    pub async fn set_model_option(&mut self, model: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.call("session/set_config_option", json!({
            "key": "model",
            "value": model
        })).await?;
        
        Ok(())
    }
    
    pub async fn send_request(&mut self, content: &str) -> Result<String, Box<dyn std::error::Error>> {
        let result = self.call("textDocument/complete", json!({
            "text": content
        })).await?;
        
        Ok(result.to_string())
    }
    
    pub async fn invoke_subagent(&mut self, agent_name: &str, task: &str) -> Result<String, Box<dyn std::error::Error>> {
        let result = self.call("agent/invoke", json!({
            "agent": agent_name,
            "task": task
        })).await?;
        
        Ok(result.to_string())
    }
    
    pub fn wait_for_exit(&mut self, _timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.process.try_wait()?;
        // In a real implementation, we'd wait with timeout
        Ok(())
    }
    
    pub async fn handshake(&mut self, _timeout: Duration) -> Result<String, Box<dyn std::error::Error>> {
        // Initialize
        self.call("initialize", json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        })).await?;
        
        // Create session
        let session_result = self.call("session/new", json!({
            "cwd": std::env::current_dir().unwrap().to_str().unwrap(),
            "mcpServers": [],
        })).await?;
        
        let session_id = session_result.get("sessionId")
            .and_then(|v| v.as_str())
            .ok_or("No sessionId in response")?
            .to_string();
        
        Ok(session_id)
    }
    
    pub fn read_message(&mut self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        Ok(serde_json::from_str(&line)?)
    }
    
    pub async fn send_request_and_wait(&mut self, method: &str, params: Value, timeout: Duration) -> Result<RpcResponse, Box<dyn std::error::Error>> {
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
    ) -> Result<(Vec<super::plan_types::PlanNotification>, RpcResponse), Box<dyn std::error::Error>> {
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
                        if let Ok(plan_notif) = serde_json::from_value::<super::plan_types::PlanNotification>(update.clone()) {
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

    pub fn send_raw(&mut self, raw: &str) -> Result<(), Box<dyn std::error::Error>> {
        {
            let mut writer = self.writer.lock().unwrap();
            writeln!(writer, "{}", raw)?;
            writer.flush()?;
        }
        Ok(())
    }
}

impl Drop for AcpChild {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}