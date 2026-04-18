//! E2E test infrastructure for loom-acp
//!
//! Spawns the loom-acp binary as a subprocess and communicates via JSON-RPC over stdin/stdout.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};
use std::{io, thread};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

/// E2E test helper that spawns loom-acp as a subprocess and communicates via JSON-RPC
pub struct AcpChild {
    child: Child,
    stdin: ChildStdin,
    stdout_reader: BufReader<ChildStdout>,
    next_id: u64,
}

#[derive(Debug, Serialize)]
struct RpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: Value,
}

#[derive(Debug, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
}

impl AcpChild {
    /// Spawn loom-acp as a subprocess
    pub fn spawn(log_file: Option<&Path>) -> io::Result<Self> {
        let mut cmd = Command::new("cargo");
        cmd.args(["run", "--bin", "loom-acp", "--"]);

        if let Some(log_file) = log_file {
            cmd.args(["--log-file", log_file.to_str().unwrap()]);
        }

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = cmd.spawn()?;

        let stdin = child.stdin.take().expect("stdin should be available");
        let stdout = child.stdout.take().expect("stdout should be available");
        let stdout_reader = BufReader::new(stdout);

        Ok(Self {
            child,
            stdin,
            stdout_reader,
            next_id: 1,
        })
    }

    /// Send JSON-RPC request and return the request id
    pub fn send_request(&mut self, method: &str, params: Value) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;

        let request = RpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        let json = serde_json::to_string(&request)?;
        writeln!(self.stdin, "{}", json)?;
        self.stdin.flush()?;

        Ok(id)
    }

    /// Send JSON-RPC notification (no id, no response expected)
    pub fn send_notification(&mut self, method: &str, params: Value) -> io::Result<()> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        let json = serde_json::to_string(&notification)?;
        writeln!(self.stdin, "{}", json)?;
        self.stdin.flush()
    }

    /// Read one JSON-RPC message (response or notification)
    pub fn read_message(&mut self) -> io::Result<Value> {
        let mut line = String::new();

        // Skip empty lines
        loop {
            line.clear();
            let bytes_read = self.stdout_reader.read_line(&mut line)?;
            if bytes_read > 0 && !line.trim().is_empty() {
                break;
            }
            if bytes_read == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stdout closed"));
            }
        }

        let value: Value = serde_json::from_str(line.trim())?;
        Ok(value)
    }

    /// Wait for a response with the given id (with timeout)
    pub fn wait_for_response(&mut self, id: u64, timeout: Duration) -> io::Result<RpcResponse> {
        let start = Instant::now();

        while start.elapsed() < timeout {
            let message = self.read_message()?;

            // Check if this is a response (has "id" field)
            if let Some(msg_id) = message.get("id").and_then(|v| v.as_u64()) {
                if msg_id == id {
                    let response: RpcResponse = serde_json::from_value(message)?;
                    return Ok(response);
                }
            }
        }

        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("timeout waiting for response id {}", id),
        ))
    }

    /// Wait for a notification with the given method (with timeout)
    pub fn wait_for_notification(
        &mut self,
        method: &str,
        timeout: Duration,
    ) -> io::Result<RpcNotification> {
        let start = Instant::now();

        while start.elapsed() < timeout {
            let message = self.read_message()?;

            // Check if this is a notification (no "id" field, has "method")
            if message.get("id").is_none() {
                if let Some(notif_method) = message.get("method").and_then(|v| v.as_str()) {
                    if notif_method == method {
                        let notification: RpcNotification = serde_json::from_value(message)?;
                        return Ok(notification);
                    }
                }
            }
        }

        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("timeout waiting for notification method {}", method),
        ))
    }

    /// Send request and wait for response
    pub fn send_request_and_wait(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> io::Result<RpcResponse> {
        let id = self.send_request(method, params)?;
        self.wait_for_response(id, timeout)
    }

    /// Run complete handshake: initialize -> session/new
    pub fn handshake(&mut self, timeout: Duration) -> io::Result<String> {
        // Initialize
        let init_response = self.send_request_and_wait(
            "initialize",
            serde_json::json!({
                "protocolVersion": 1,
            }),
            timeout,
        )?;

        assert!(
            init_response.error.is_none(),
            "initialize failed: {:?}",
            init_response.error
        );

        // Start session
        let new_session_response = self.send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir()?.to_str().unwrap(),
            }),
            timeout,
        )?;

        let session_id = new_session_response
            .result
            .and_then(|r| r.get("sessionId").and_then(|s| s.as_str().map(|s| s.to_string())))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no session_id in response"))?;

        Ok(session_id)
    }

    /// Kill the child process
    pub fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }

    /// Wait for child to exit
    pub fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
        self.child.wait()
    }
}

impl Drop for AcpChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_acp() {
        let mut acp = AcpChild::spawn(None).expect("should spawn loom-acp");
        let _ = acp.kill();
    }
}
