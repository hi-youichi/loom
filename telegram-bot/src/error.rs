//! Unified error types for telegram-bot
//!
//! This module provides a centralized error handling system using thiserror.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BotError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Network error: {0}")]
    Network(#[from] teloxide::RequestError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Channel send error: {0}")]
    ChannelSend(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Download error: {0}")]
    Download(#[from] teloxide::DownloadError),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type Result<T> = std::result::Result<T, BotError>;
