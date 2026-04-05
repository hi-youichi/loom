use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Environment variable not found: {0}")]
    EnvVarNotFound(String),

    #[error("No bots configured")]
    NoBots,

    #[error("Bot '{0}' has no token configured")]
    MissingToken(String),
}
