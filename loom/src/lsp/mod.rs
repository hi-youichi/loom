//! LSP (Language Server Protocol) integration for Loom.
//!
//! This module provides LSP-based code intelligence capabilities including:
//! - Code completion
//! - Diagnostics (errors, warnings)
//! - Go to definition
//! - Find references
//! - Hover information
//! - Document symbols
//!
//! # Architecture
//!
//! The LSP integration consists of three layers:
//! - **LSP Client**: Low-level communication with language servers
//! - **LSP Manager**: Manages multiple language server instances
//! - **LSP Tool**: High-level Tool trait implementation for agent use
//!
//! # Example
//!
//! ```rust,ignore
//! use loom::lsp::{LspManager, LspConfig};
//! use std::sync::Arc;
//! use tokio::sync::RwLock;
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = LspConfig::default();
//!     let manager = LspManager::new(config);
//!     let manager = Arc::new(RwLock::new(manager));
//!     
//!     // Get completions
//!     let completions = manager.write().await
//!         .get_completion("src/main.rs", 10, 5)
//!         .await
//!         .unwrap();
//! }
//! ```

pub mod client;
pub mod manager;
pub mod sync;
pub mod cache;
pub mod workspace;
pub mod performance;
pub mod error_recovery;
pub mod installer;
pub mod types;
// mod examples; // Disabled: examples.rs uses `loom::` which is not available within the loom crate

#[cfg(test)]
mod tests;

#[cfg(test)]
mod mock_server;

#[cfg(test)]
mod integration_tests;

pub use client::LspClient;
pub use manager::{LspManager, LspManagerError};
pub use sync::DocumentState;
pub use cache::DiagnosticCache;
pub use workspace::Workspace;
pub use performance::PerformanceMonitor;
// ErrorRecovery is used internally
// LanguageServerInstaller is used internally
pub use types::*;

// Re-export LSP types from config
pub use env_config::{LspServerConfig};

/// Result type for LSP operations
pub type LspResult<T> = Result<T, LspManagerError>;
