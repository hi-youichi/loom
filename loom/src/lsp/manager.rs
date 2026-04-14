//! LSP Manager: manages multiple language server instances.
//!
//! Responsibilities:
//! - Start/stop language servers based on file types
//! - Route requests to appropriate servers
//! - Cache server capabilities and diagnostics
//! - Handle server lifecycle and error recovery

use crate::lsp::cache::DiagnosticCache;
use crate::lsp::client::LspClient;
use dashmap::DashMap;
use env_config::LspServerConfig;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

#[derive(Debug, Error)]
pub enum LspManagerError {
    #[error("No language server configured for language: {0}")]
    NoServerForLanguage(String),

    #[error("No language server configured for file: {0}")]
    NoServerForFile(String),

    #[error("Failed to start language server: {0}")]
    StartFailed(#[source] crate::lsp::client::LspClientError),

    #[error("Language server not initialized: {0}")]
    NotInitialized(String),

    #[error("Client error: {0}")]
    ClientError(#[from] crate::lsp::client::LspClientError),

    #[error("Invalid file path: {0}")]
    InvalidPath(String),
}

/// LSP Manager: coordinates multiple language server instances.
pub struct LspManager {
    /// Language server instances keyed by language ID
    clients: DashMap<String, Arc<RwLock<LspClient>>>,
    /// Server configurations
    configs: Vec<LspServerConfig>,
    /// Diagnostic cache
    #[allow(dead_code)]
    diagnostic_cache: DiagnosticCache,
    /// Extension to language mapping
    extension_map: DashMap<String, String>,
}

impl LspManager {
    /// Create a new LSP manager with default configuration.
    pub async fn new() -> Result<Self, LspManagerError> {
        let configs = env_config::get_default_lsp_servers();
        Self::with_configs(configs).await
    }

    /// Build manager state from server configs (sync; safe to call from any thread).
    pub fn from_configs(configs: Vec<LspServerConfig>) -> Self {
        let extension_map = DashMap::new();

        for config in &configs {
            for pattern in &config.file_patterns {
                if let Some(ext) = pattern.strip_prefix("*.") {
                    extension_map.insert(ext.to_string(), config.language.clone());
                }
            }
        }

        Self {
            clients: DashMap::new(),
            configs,
            diagnostic_cache: DiagnosticCache::new(),
            extension_map,
        }
    }

    /// Create an LSP manager with custom configurations.
    pub async fn with_configs(configs: Vec<LspServerConfig>) -> Result<Self, LspManagerError> {
        Ok(Self::from_configs(configs))
    }

    /// Detect language from file path.
    pub fn detect_language(&self, file_path: &Path) -> Option<String> {
        file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| self.extension_map.get(ext).map(|s| s.clone()))
    }

    /// Get or create a client for the specified language.
    pub async fn get_client(
        &self,
        language: &str,
    ) -> Result<Arc<RwLock<LspClient>>, LspManagerError> {
        // Check if client already exists
        if let Some(client) = self.clients.get(language) {
            return Ok(client.clone());
        }

        // Find configuration for this language
        let config = self
            .configs
            .iter()
            .find(|c| c.language == language)
            .ok_or_else(|| LspManagerError::NoServerForLanguage(language.to_string()))?
            .clone();

        // Start new client
        info!(
            "Starting language server for {}: {}",
            language, config.command
        );
        let root_uri = None; // TODO: Use workspace root
        let mut client = LspClient::start(&config.command, &config.args, root_uri)
            .await
            .map_err(LspManagerError::StartFailed)?;

        // Initialize the server
        client
            .initialize()
            .await
            .map_err(LspManagerError::StartFailed)?;

        // Store in cache
        let client_arc = Arc::new(RwLock::new(client));
        self.clients
            .insert(language.to_string(), client_arc.clone());

        Ok(client_arc)
    }

    /// Get client for a file path (auto-detects language).
    pub async fn get_client_for_file(
        &self,
        file_path: &Path,
    ) -> Result<Arc<RwLock<LspClient>>, LspManagerError> {
        let language = self
            .detect_language(file_path)
            .ok_or_else(|| LspManagerError::NoServerForFile(file_path.display().to_string()))?;

        self.get_client(&language).await
    }

    /// Open a document in the appropriate language server.
    pub async fn open_document(
        &self,
        file_path: &Path,
        content: &str,
    ) -> Result<(), LspManagerError> {
        let language = self
            .detect_language(file_path)
            .ok_or_else(|| LspManagerError::NoServerForFile(file_path.display().to_string()))?;

        let client = self.get_client(&language).await?;
        let mut client = client.write().await;

        let uri = path_to_uri(file_path)?;
        client
            .open_document(&uri, &language, content)
            .await
            .map_err(Into::into)
    }

    /// Get completions at a position.
    pub async fn completion(
        &self,
        file_path: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<lsp_types::CompletionItem>, LspManagerError> {
        let client = self.get_client_for_file(file_path).await?;
        let mut client = client.write().await;

        let uri = path_to_uri(file_path)?;
        client
            .completion(&uri, line, character)
            .await
            .map_err(Into::into)
    }

    /// Get diagnostics for a file.
    pub async fn diagnostics(
        &self,
        file_path: &Path,
    ) -> Result<Vec<lsp_types::Diagnostic>, LspManagerError> {
        let client = self.get_client_for_file(file_path).await?;
        let client = client.read().await;

        let uri = path_to_uri(file_path)?;
        client.diagnostics(&uri).await.map_err(Into::into)
    }

    /// Go to definition at a position.
    pub async fn goto_definition(
        &self,
        file_path: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<lsp_types::Location>, LspManagerError> {
        let client = self.get_client_for_file(file_path).await?;
        let mut client = client.write().await;

        let uri = path_to_uri(file_path)?;
        client
            .goto_definition(&uri, line, character)
            .await
            .map_err(Into::into)
    }

    /// Find references at a position.
    pub async fn find_references(
        &self,
        file_path: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<lsp_types::Location>, LspManagerError> {
        let client = self.get_client_for_file(file_path).await?;
        let mut client = client.write().await;

        let uri = path_to_uri(file_path)?;
        client
            .find_references(&uri, line, character)
            .await
            .map_err(Into::into)
    }

    /// Get hover information at a position.
    pub async fn hover(
        &self,
        file_path: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<lsp_types::Hover>, LspManagerError> {
        let client = self.get_client_for_file(file_path).await?;
        let mut client = client.write().await;

        let uri = path_to_uri(file_path)?;
        client
            .hover(&uri, line, character)
            .await
            .map_err(Into::into)
    }

    /// Get document symbols.
    pub async fn document_symbols(
        &self,
        file_path: &Path,
    ) -> Result<Option<lsp_types::DocumentSymbolResponse>, LspManagerError> {
        let client = self.get_client_for_file(file_path).await?;
        let mut client = client.write().await;

        let uri = path_to_uri(file_path)?;
        client.document_symbols(&uri).await.map_err(Into::into)
    }

    /// Shutdown all language servers.
    pub async fn shutdown_all(&self) {
        info!("Shutting down all language servers");

        for entry in self.clients.iter() {
            let language = entry.key();
            let client = entry.value();

            if let Ok(mut client) = client.try_write() {
                if let Err(e) = client.shutdown().await {
                    warn!("Failed to shutdown language server for {}: {}", language, e);
                }
            }
        }

        self.clients.clear();
    }

    /// Get list of active language servers.
    pub fn active_servers(&self) -> Vec<String> {
        self.clients
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }
}

impl Default for LspManager {
    fn default() -> Self {
        Self::from_configs(env_config::get_default_lsp_servers())
    }
}

/// Convert file path to URI.
fn path_to_uri(path: &Path) -> Result<lsp_types::Url, LspManagerError> {
    lsp_types::Url::from_file_path(path)
        .map_err(|_| LspManagerError::InvalidPath(path.display().to_string()))
}

impl Drop for LspManager {
    fn drop(&mut self) {
        // Attempt to shutdown servers (best effort)
        debug!("Dropping LspManager");
    }
}
