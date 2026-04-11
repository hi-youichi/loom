//! LSP Client: manages communication with a language server process.
//!
//! Handles:
//! - Starting and stopping language server processes
//! - JSON-RPC protocol encoding/decoding
//! - stdio-based communication
//! - Request/response correlation

use std::{
    collections::HashMap,
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
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, oneshot, RwLock},
};
use tracing::{debug, error, info, warn};

use crate::lsp::cache::DiagnosticCache;

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
    process: Option<Child>,

    stdin: tokio::process::ChildStdin,

    pending_requests: PendingRequests,

    shutdown_tx: mpsc::Sender<()>,

    capabilities: Arc<RwLock<Option<InitializeResult>>>,

    root_uri: Option<Url>,

    diagnostics_cache: Arc<DiagnosticCache>,
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
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
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
        let diagnostics_cache = Arc::new(DiagnosticCache::new());

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);

        let pending_requests_clone = pending_requests.clone();
        let capabilities_clone = capabilities.clone();
        let diagnostics_cache_clone = diagnostics_cache.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);

            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => break,
                    result = read_lsp_message(&mut reader) => {
                        match result {
                            None => {
                                debug!("LSP server closed connection");
                                break;
                            }
                            Some(body_str) => {
                                if let Err(e) = Self::handle_response(
                                    &body_str,
                                    &pending_requests_clone,
                                    &capabilities_clone,
                                    &diagnostics_cache_clone,
                                ).await {
                                    error!("Failed to handle LSP response: {}", e);
                                }
                            }
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
            diagnostics_cache,
        })
    }
    
    /// Initialize the language server
    pub async fn initialize(&mut self) -> LspResult<InitializeResult> {
        let params = InitializeParams {
            #[allow(deprecated)]
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
    pub async fn diagnostics(&self, uri: &Url) -> LspResult<Vec<Diagnostic>> {
        Ok(self.diagnostics_cache.get_latest(uri).await.unwrap_or_default())
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
        
        self.stdin.write_all(header.as_bytes()).await?;
        self.stdin.write_all(content.as_bytes()).await?;
        self.stdin.flush().await?;
        
        debug!("Sent LSP message: {}", message["method"].as_str().unwrap_or("unknown"));
        Ok(())
    }
    
    /// Handle incoming response from server
    async fn handle_response(
        line: &str,
        pending_requests: &PendingRequests,
        _capabilities: &Arc<RwLock<Option<InitializeResult>>>,
        diagnostics_cache: &Arc<DiagnosticCache>,
    ) -> LspResult<()> {
        if line.is_empty() {
            return Ok(());
        }

        if line.starts_with("Content-Length:") {
            return Ok(());
        }

        let message: Value = serde_json::from_str(line)?;

        if let Some(id) = message.get("id").and_then(|id| id.as_u64()) {
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
            match method {
                "textDocument/publishDiagnostics" => {
                    debug!("Received diagnostics notification");
                    if let Some(params) = message.get("params") {
                        if let Some(uri_str) = params.get("uri").and_then(|u| u.as_str()) {
                            if let Ok(uri) = Url::parse(uri_str) {
                                let diags: Vec<Diagnostic> =
                                    params.get("diagnostics").and_then(|d| serde_json::from_value(d.clone()).ok()).unwrap_or_default();
                                let version = params.get("version").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                diagnostics_cache.put(uri, version, diags).await;
                            }
                        }
                    }
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
                process.wait()
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
        } else if let Some(_location) = result.get("uri") {
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

        let max_retries = 3;
        for attempt in 0..=max_retries {
            match self.request::<_, Value>("textDocument/references", &params).await {
                Ok(result) => {
                    let locations: Vec<lsp_types::Location> = serde_json::from_value(result)?;
                    return Ok(locations);
                }
                Err(LspClientError::Protocol(msg))
                    if msg.contains("content modified") || msg.contains("ContentModified") =>
                {
                    if attempt < max_retries {
                        debug!("find_references: content modified, retrying ({}/{})", attempt + 1, max_retries);
                        tokio::time::sleep(Duration::from_millis(200 * (attempt as u64 + 1))).await;
                        continue;
                    }
                    return Err(LspClientError::Protocol(msg));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(Vec::new())
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
    ) -> LspResult<Option<lsp_types::DocumentSymbolResponse>> {
        let params = json!({
            "textDocument": { "uri": uri }
        });

        let result: Value = self.request("textDocument/documentSymbol", params).await?;

        if result.is_null() {
            return Ok(None);
        }

        let response: lsp_types::DocumentSymbolResponse = serde_json::from_value(result)?;
        Ok(Some(response))
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

async fn read_lsp_message<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Option<String> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                debug!("read_lsp_message: EOF on header read");
                return None;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    break;
                }
                if let Some(length) = trimmed.strip_prefix("Content-Length: ") {
                    content_length = Some(length.parse().unwrap_or(0));
                }
            }
            Err(e) => {
                debug!("read_lsp_message: header read error: {}", e);
                return None;
            }
        }
    }
    
    let length = content_length?;
    if length == 0 {
        return Some(String::new());
    }
    
    let mut body = vec![0u8; length];
    match reader.read_exact(&mut body).await {
        Ok(_n) => {
            let body_str = String::from_utf8_lossy(&body).into_owned();
            debug!("read_lsp_message: received {} bytes", body_str.len());
            Some(body_str)
        }
        Err(e) => {
            debug!("read_lsp_message: body read error: {}", e);
            None
        }
    }
}

impl Drop for LspClient {
    #[allow(clippy::let_underscore_future)]
    fn drop(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
            warn!("LSP server process killed");
        }
    }
}
