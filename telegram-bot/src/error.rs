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

    #[error("Agent run error: {0}")]
    AgentRun(#[from] loom::cli_run::RunError),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Download error: {0}")]
    Download(#[from] teloxide::DownloadError),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type Result<T> = std::result::Result<T, BotError>;
