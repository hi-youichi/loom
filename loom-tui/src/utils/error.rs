//! Error handling for Loom TUI

use thiserror::Error;

/// Result type for Loom TUI operations
pub type Result<T> = std::result::Result<T, LoomTuiError>;

/// Error types for Loom TUI
#[derive(Error, Debug)]
pub enum LoomTuiError {
    #[error("Terminal initialization failed: {0}")]
    TerminalInitFailed(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    #[error("TUI rendering error: {0}")]
    RenderError(String),
    
    #[error("Agent integration error: {0}")]
    AgentError(String),
    
    #[error("Session management error: {0}")]
    SessionError(String),
    
    #[error("Input handling error: {0}")]
    InputError(String),
    
    #[error("Event processing error: {0}")]
    EventError(String),
    
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),
    
    #[error("TOML parsing error: {0}")]
    TomlError(#[from] toml::de::Error),
    
    #[error("UTF-8 conversion error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),
    
    #[error("Async error: {0}")]
    AsyncError(String),
    
    #[error("Crossterm error: {0}")]
    CrosstermError(String),
    
    #[error("Configuration file not found: {0}")]
    ConfigFileNotFound(String),
    
    #[error("Invalid terminal size: {0}")]
    InvalidTerminalSize(String),
    
    #[error("Channel error: {0}")]
    ChannelError(String),
}

impl From<anyhow::Error> for LoomTuiError {
    fn from(err: anyhow::Error) -> Self {
        LoomTuiError::AgentError(err.to_string())
    }
}

impl From<tokio::task::JoinError> for LoomTuiError {
    fn from(err: tokio::task::JoinError) -> Self {
        LoomTuiError::AsyncError(err.to_string())
    }
}

impl From<clap::error::Error> for LoomTuiError {
    fn from(err: clap::error::Error) -> Self {
        LoomTuiError::ConfigError(err.to_string())
    }
}

/// Error context for better debugging
#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub operation: String,
    pub component: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub additional_info: Option<String>,
}

impl ErrorContext {
    pub fn new(operation: &str, component: &str) -> Self {
        Self {
            operation: operation.to_string(),
            component: component.to_string(),
            timestamp: chrono::Utc::now(),
            additional_info: None,
        }
    }
    
    pub fn with_info(mut self, info: String) -> Self {
        self.additional_info = Some(info);
        self
    }
}

/// Enhanced error with context
#[derive(Error, Debug)]
#[error("{context}: {source}")]
pub struct EnhancedError {
    pub context: ErrorContext,
    #[source]
    pub source: LoomTuiError,
}

impl EnhancedError {
    pub fn new(context: ErrorContext, source: LoomTuiError) -> Self {
        Self { context, source }
    }
}

/// Error handling utilities
pub struct ErrorHandler;

impl ErrorHandler {
    pub fn wrap_error<T>(context: ErrorContext, result: Result<T>) -> Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(source) => Err(EnhancedError::new(context, source).into()),
        }
    }
    
    pub fn log_error(error: &LoomTuiError) {
        tracing::error!("Loom TUI Error: {}", error);
    }
    
    pub fn log_enhanced_error(error: &EnhancedError) {
        tracing::error!(
            operation = %error.context.operation,
            component = %error.context.component,
            timestamp = %error.context.timestamp,
            additional_info = %error.context.additional_info.as_deref().unwrap_or(""),
            error = %error.source
        );
    }
}