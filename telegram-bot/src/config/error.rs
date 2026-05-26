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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_error_io_display() {
        let e = ConfigError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "file"));
        assert!(e.to_string().contains("IO error"));
    }

    #[test]
    fn config_error_env_var_display() {
        let e = ConfigError::EnvVarNotFound("BOT_TOKEN".into());
        assert!(e.to_string().contains("BOT_TOKEN"));
    }

    #[test]
    fn config_error_no_bots_display() {
        let e = ConfigError::NoBots;
        assert_eq!(e.to_string(), "No bots configured");
    }

    #[test]
    fn config_error_missing_token_display() {
        let e = ConfigError::MissingToken("mybot".into());
        assert!(e.to_string().contains("mybot"));
    }

    #[test]
    fn config_error_toml_from() {
        let e = ConfigError::from(toml::from_str::<toml::Value>("{invalid").unwrap_err());
        match e {
            ConfigError::Toml(_) => {}
            _ => panic!("expected Toml variant"),
        }
    }
}
