use std::process::{Child, Stdio, Command};
use std::path::Path;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use super::RpcResponse;

// Simple mock server for testing
#[allow(dead_code)]
pub struct MockAcpServer {
    responses: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl MockAcpServer {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(Vec::new())),
        }
    }
    
    #[allow(dead_code)]
    pub async fn mount_tool_call_response(&self, responses: &[ToolCallResponse]) {
        let mut stored = self.responses.lock().unwrap();
        let base_len = stored.len();
        for (i, response) in responses.iter().enumerate() {
            stored.push(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "toolCallId": format!("tool_{}", base_len + i),
                    "toolName": response.tool_name,
                    "parameters": response.parameters
                }
            }));
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallResponse {
    pub tool_name: String,
    pub parameters: serde_json::Value,
}

pub struct AcpChild {
    process: Child,
    reader: BufReader<std::process::ChildStdout>,
    writer: Arc<Mutex<std::process::ChildStdin>>,
    request_id: Arc<Mutex<u64>>,
}

#[allow(dead_code)]
impl AcpChild {
    pub fn spawn(home: Option<&Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let actual_home = if let Some(h) = home {
            h.to_path_buf()
        } else {
            // 如果没有提供home路径，使用当前目录
            std::env::current_dir().expect("Failed to get current directory")
        };
        
        let bin = env!("CARGO_BIN_EXE_loom-acp");
        let mut process = Command::new(bin)
            .env("LOOM_HOME", &actual_home)
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
        })
    }
    
    pub async fn spawn_with_mock() -> Result<(Self, MockAcpServer), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;
        let home = temp_dir.path();
        
        let acp = Self::spawn(Some(home))?;
        let mock = MockAcpServer::new();
        
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
}

impl Drop for AcpChild {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}