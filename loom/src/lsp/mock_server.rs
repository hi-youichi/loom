//! Mock LSP server for testing purposes.
//!
//! This module provides a simple mock LSP server that can be used in tests
//! without requiring actual language server installations.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Mock LSP server that responds to basic LSP requests.
pub struct MockLspServer {
    responses: Arc<Mutex<HashMap<String, Value>>>,
    running: Arc<Mutex<bool>>,
}

impl MockLspServer {
    /// Create a new mock LSP server.
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(Mutex::new(false)),
        }
    }

    /// Set a custom response for a specific method.
    pub fn set_response(&self, method: &str, response: Value) {
        self.responses
            .lock()
            .unwrap()
            .insert(method.to_string(), response);
    }

    /// Start the mock server on a separate thread.
    pub fn start(&self) -> MockServerHandle {
        let running = Arc::clone(&self.running);
        let running_clone = Arc::clone(&running);
        let responses = Arc::clone(&self.responses);

        *running.lock().unwrap() = true;

        let handle = thread::spawn(move || {
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();

            let mut reader = BufReader::new(stdin.lock());
            let mut writer = BufWriter::new(stdout.lock());

            while *running.lock().unwrap() {
                // Read content length
                let mut content_length_line = String::new();
                if reader.read_line(&mut content_length_line).is_err() {
                    break;
                }

                if !content_length_line.starts_with("Content-Length:") {
                    continue;
                }

                let content_length: usize = content_length_line
                    .split(':')
                    .nth(1)
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);

                // Read empty line
                let mut empty_line = String::new();
                if reader.read_line(&mut empty_line).is_err() {
                    break;
                }

                // Read content
                let mut content = vec![0u8; content_length];
                if reader.read_exact(&mut content).is_err() {
                    break;
                }

                let request: Value = match serde_json::from_slice(&content) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let method = request["method"].as_str().unwrap_or("");

                let response = if method == "initialize" {
                    json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "result": {
                            "capabilities": {
                                "textDocumentSync": 1,
                                "completionProvider": {
                                    "resolveProvider": false,
                                    "triggerCharacters": ["."]
                                },
                                "definitionProvider": true,
                                "referencesProvider": true,
                                "hoverProvider": true,
                                "documentSymbolProvider": true
                            },
                            "serverInfo": {
                                "name": "mock-lsp-server",
                                "version": "1.0.0"
                            }
                        }
                    })
                } else if let Some(custom_response) = responses.lock().unwrap().get(method) {
                    json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "result": custom_response
                    })
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "error": {
                            "code": -32601,
                            "message": "Method not found"
                        }
                    })
                };

                let response_str = serde_json::to_string(&response).unwrap();
                write!(
                    writer,
                    "Content-Length: {}\r\n\r\n{}",
                    response_str.len(),
                    response_str
                )
                .unwrap();
                writer.flush().unwrap();
            }
        });

        MockServerHandle {
            running: running_clone,
            handle: Some(handle),
        }
    }
}

/// Handle to stop the mock server.
pub struct MockServerHandle {
    running: Arc<Mutex<bool>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MockServerHandle {
    /// Stop the mock server.
    pub fn stop(&mut self) {
        *self.running.lock().unwrap() = false;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MockServerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_server_creation() {
        let server = MockLspServer::new();
        server.set_response(
            "textDocument/completion",
            json!({
                "items": [
                    {
                        "label": "test",
                        "kind": 3,
                        "detail": "Test completion"
                    }
                ]
            }),
        );

        let responses = server.responses.lock().unwrap();
        assert!(responses.contains_key("textDocument/completion"));
    }
}
