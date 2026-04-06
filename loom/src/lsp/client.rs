//! LSP Client: manages communication with a language server process.
//!
//! Handles:
//! - Starting and stopping language server processes
//! - JSON-RPC protocol encoding/decoding
//! - stdio-based communication
//! - Request/response correlation

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use lsp_types::{
    CompletionItem, CompletionParams, Diagnostic, DidOpenTextDocumentParams,
    InitializeParams, InitializeResult, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Url,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{debug, error, info, warn};

/// LSP result type
pub type LspResult<T> = Result<T, LspClientError>;

/// LSP client error type
#[derive(Debug, thiserror::Error)]
pub enum LspClientError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Protocol error: {0}")]
    Protocol(String),
    
    #[error("Server not initialized")]
    NotInitialized,
    
    #[error("Server crashed: {0}")]
    ServerCrashed(String),
    
    #[error("Request timeout")]
    Timeout,
    
    #[error("Request cancelled")]
    Cancelled,
}



/// Unique request ID generator
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Pending request tracker
type PendingRequests = Arc<RwLock<HashMap<u64, oneshot::Sender<LspResult<Value>>>>>;

/// LSP Client managing a single language server process
pub struct LspClient {
    /// Language server process
    process: Option<Child>,
    
    /// Stdin for sending requests
    stdin: ChildStdin,
    
    /// Pending requests waiting for responses
    pending_requests: PendingRequests,
    
    /// Channel to signal shutdown
    shutdown_tx: mpsc::Sender<()>,
    
    /// Server capabilities (set after initialization)
    capabilities: Arc<RwLock<Option<InitializeResult>>>,
    
    /// Workspace root URI
    root_uri: Option<Url>,
}

impl LspClient {
    /// Start a new LSP client with the given command
    pub async fn start(
        command: &str,
        args: &[String],
        root_uri: Option<Url>,
    ) -> LspResult<Self> {
        info!("Starting LSP server: {} {:?}", command, args);
        
        let mut process = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| LspClientError::Protocol(format!("Failed to start {}: {}", command, e)))?;
        
        let stdin = process.stdin.take().ok_or_else(|| {
            LspClientError::Protocol("Failed to get stdin".to_string())
        })?;
        
        let stdout = process.stdout.take().ok_or_else(|| {
            LspClientError::Protocol("Failed to get stdout".to_string())
        })?;
        
        let pending_requests = Arc::new(RwLock::new(HashMap::new()));
        let capabilities = Arc::new(RwLock::new(None));
        
        // Channel for shutdown signaling
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        
        // Spawn background task to read responses
       let pending_requests_clone = pending_requests.clone();
       let capabilities_clone = capabilities.clone();
       tokio::spawn(async move {
           let mut reader = BufReader::new(stdout);
           
           loop {
               // Check for shutdown signal
               if shutdown_rx.try_recv().is_ok() {
                   break;
               }
               
                // Read Content-Length header
                let mut content_length: Option<usize> = None;
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => break, // EOF
                        Ok(_) => {
                            let line = line.trim();
                            if line.is_empty() {
                                // Empty line marks end of headers
                                break;
                            }
                            if let Some(length) = line.strip_prefix("Content-Length: ") {
                                content_length = Some(length.parse().unwrap_or(0));
                            }
                        }
                        Err(e) => {
                            error!("Failed to read header from LSP server: {}", e);
                            return;
                        }
                    }
                }
                
                // Read message body
                if let Some(length) = content_length {
                    if length == 0 {
                        continue;
                    }
                    
                    let mut body = vec![0u8; length];
                    match reader.read_exact(&mut body) {
                        Ok(_) => {
                            let body_str = String::from_utf8_lossy(&body);
                            if let Err(e) = Self::handle_response(
                                &body_str,
                                &pending_requests_clone,
                                &capabilities_clone,
                            ).await {
                                error!("Failed to handle LSP response: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("Failed to read body from LSP server: {}", e);
                            break;
                        }
                    }
               }
           }
           
           debug!("LSP reader task stopped");
       });
        
        Ok(Self {
            process: Some(process),
            stdin,
            pending_requests,
            shutdown_tx,
            capabilities,
            root_uri,
        })
    }
    
    /// Initialize the language server
    pub async fn initialize(&mut self) -> LspResult<InitializeResult> {
        let params = InitializeParams {
            root_uri: self.root_uri.clone(),
            ..Default::default()
        };
        
        let result: InitializeResult = self.request("initialize", params).await?;
        
        // Store capabilities
        *self.capabilities.write().await = Some(result.clone());
        
        // Send initialized notification
        self.notify("initialized", json!({})).await?;
        
        info!("LSP server initialized successfully");
        Ok(result)
    }
    
    /// Open a text document in the language server
    pub async fn open_document(
        &mut self,
        uri: &Url,
        language_id: &str,
        text: &str,
    ) -> LspResult<()> {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: language_id.to_string(),
                version: 1,
                text: text.to_string(),
            },
        };
        
        self.notify("textDocument/didOpen", params).await
    }
    
    /// Request code completion
    pub async fn completion(
        &mut self,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> LspResult<Vec<CompletionItem>> {
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: lsp_types::Position { line, character },
            },
            context: None,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        
        let result: Value = self.request("textDocument/completion", params).await?;
        
        // Handle both CompletionList and Vec<CompletionItem>
        let items: Vec<CompletionItem> = if result.is_array() {
            serde_json::from_value(result)?
        } else if let Some(list) = result.get("items") {
            serde_json::from_value(list.clone())?
        } else {
            Vec::new()
        };
        
        Ok(items)
    }
    
    /// Get diagnostics for a document
    pub async fn diagnostics(&mut self, uri: &Url) -> LspResult<Vec<Diagnostic>> {
        // Diagnostics are typically pushed via notifications
        // This is a placeholder - actual implementation would cache diagnostics
        let params = TextDocumentIdentifier { uri: uri.clone() };
        let _result: () = self.request("textDocument/diagnostics", params).await?;
        
        // For now, return empty (diagnostics come via notifications)
        Ok(Vec::new())
    }
    
    /// Send a request and wait for response
    async fn request<T: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        params: T,
    ) -> LspResult<R> {
        let id = REQUEST_ID.fetch_add(1, Ordering::SeqCst);
        
        let (tx, rx) = oneshot::channel();
        self.pending_requests.write().await.insert(id, tx);
        
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        
        self.send_message(&request).await?;
        
        // Wait for response with timeout
        let response = tokio::time::timeout(
            Duration::from_secs(30),
            rx
        ).await
            .map_err(|_| LspClientError::Timeout)?
            .map_err(|_| LspClientError::Cancelled)?
            .map_err(|e| LspClientError::Protocol(format!("Request {} failed: {}", method, e)))?;
        
        let result: R = serde_json::from_value(response)?;
        Ok(result)
    }
    
    /// Send a notification (no response expected)
    async fn notify<T: Serialize>(
        &mut self,
        method: &str,
        params: T,
    ) -> LspResult<()> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        
        self.send_message(&notification).await
    }
    
    /// Send a JSON-RPC message
    async fn send_message(&mut self, message: &Value) -> LspResult<()> {
        let content = serde_json::to_string(message)?;
        let header = format!("Content-Length: {}\r\n\r\n", content.len());
        
        self.stdin.write_all(header.as_bytes())?;
        self.stdin.write_all(content.as_bytes())?;
        self.stdin.flush()?;
        
        debug!("Sent LSP message: {}", message["method"].as_str().unwrap_or("unknown"));
        Ok(())
    }
    
    /// Handle incoming response from server
    async fn handle_response(
        line: &str,
        pending_requests: &PendingRequests,
        capabilities: &Arc<RwLock<Option<InitializeResult>>>,
    ) -> LspResult<()> {
        if line.is_empty() {
            return Ok(());
        }
        
        // Parse headers
        if line.starts_with("Content-Length:") {
            return Ok(()); // Skip header line
        }
        
        // Try to parse as JSON-RPC message
        let message: Value = serde_json::from_str(line)?;
        
        if let Some(id) = message.get("id").and_then(|id| id.as_u64()) {
            // This is a response
            let pending = pending_requests.write().await.remove(&id);
            
            if let Some(tx) = pending {
                if let Some(error) = message.get("error") {
                    let error_msg = error["message"].as_str().unwrap_or("Unknown error");
                    tx.send(Err(LspClientError::Protocol(error_msg.to_string())))
                        .map_err(|_| LspClientError::Cancelled)?;
                } else if let Some(result) = message.get("result") {
                    tx.send(Ok(result.clone()))
                        .map_err(|_| LspClientError::Cancelled)?;
                }
            }
        } else if let Some(method) = message.get("method").and_then(|m| m.as_str()) {
            // This is a notification
            match method {
                "textDocument/publishDiagnostics" => {
                    debug!("Received diagnostics notification");
                    // TODO: Cache diagnostics
                }
                _ => {
                    debug!("Received notification: {}", method);
                }
            }
        }
        
        Ok(())
    }
    
    /// Shutdown the language server
    pub async fn shutdown(&mut self) -> LspResult<()> {
        // Send shutdown request
        let _: Value = self.request("shutdown", json!({})).await?;
        
        // Send exit notification
        self.notify("exit", json!({})).await?;
        
        // Signal background task to stop
        let _ = self.shutdown_tx.send(()).await;
        
        // Wait for process to exit
        if let Some(mut process) = self.process.take() {
            tokio::time::timeout(
                Duration::from_secs(5),
                async { process.wait() }
            ).await
                .map_err(|_| LspClientError::Timeout)?
                .map_err(|e| LspClientError::Protocol(format!("Process wait failed: {}", e)))?;
        }
        
        info!("LSP server shut down successfully");
        Ok(())
    }
    
    /// Go to definition
    pub async fn goto_definition(
        &mut self,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> LspResult<Vec<lsp_types::Location>> {
        let params = lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri.clone() },
            position: lsp_types::Position { line, character },
        };
        
        let result: Value = self.request("textDocument/definition", params).await?;
        
        // Handle both Location and LocationLink
        let locations: Vec<lsp_types::Location> = if result.is_array() {
            serde_json::from_value(result)?
        } else if let Some(location) = result.get("uri") {
            vec![serde_json::from_value(result)?]
        } else {
            Vec::new()
        };
        
        Ok(locations)
    }
    
    /// Find references
    pub async fn find_references(
        &mut self,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> LspResult<Vec<lsp_types::Location>> {
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": true }
        });
        
        let result: Value = self.request("textDocument/references", params).await?;
        let locations: Vec<lsp_types::Location> = serde_json::from_value(result)?;
        
        Ok(locations)
    }
    
    /// Get hover information
    pub async fn hover(
        &mut self,
        uri: &Url,
        line: u32,
        character: u32,
    ) -> LspResult<Option<lsp_types::Hover>> {
        let params = lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri.clone() },
            position: lsp_types::Position { line, character },
        };
        
        let result: Value = self.request("textDocument/hover", params).await?;
        
        if result.is_null() {
            Ok(None)
        } else {
            let hover: lsp_types::Hover = serde_json::from_value(result)?;
            Ok(Some(hover))
        }
    }
    
    /// Get document symbols
    pub async fn document_symbols(
        &mut self,
        uri: &Url,
    ) -> LspResult<Vec<lsp_types::DocumentSymbolResponse>> {
        let params = json!({
            "textDocument": { "uri": uri }
        });
        
        let result: Value = self.request("textDocument/documentSymbol", params).await?;
        let symbols: Vec<lsp_types::DocumentSymbolResponse> = serde_json::from_value(result)?;
        
        Ok(symbols)
    }
    
    /// Add workspace folder
    pub async fn add_workspace(&mut self, _uri: &Url) -> LspResult<()> {
        // TODO: Implement workspace folder support
        Ok(())
    }
    
    /// Remove workspace folder
    pub async fn remove_workspace(&mut self, _uri: &Url) -> LspResult<()> {
        // TODO: Implement workspace folder support
        Ok(())
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
            warn!("LSP server process killed");
        }
    }
}
