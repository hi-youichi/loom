use thiserror::Error;

#[derive(Debug, Error)]
pub enum BotError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Network error: {0}")]
    Network(#[from] teloxide::RequestError),

    #[error("Download error: {0}")]
    Download(#[from] teloxide::DownloadError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Agent run error: {0}")]
    AgentRun(#[from] loom::agent_run::RunError),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("{0}")]
    Unknown(String),
}

pub type Result<T> = std::result::Result<T, BotError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bot_error_config_display() {
        let e = BotError::Config("bad config".into());
        assert_eq!(e.to_string(), "Configuration error: bad config");
    }

    #[test]
    fn bot_error_agent_display() {
        let e = BotError::Agent("timeout".into());
        assert_eq!(e.to_string(), "Agent error: timeout");
    }

    #[test]
    fn bot_error_unknown_display() {
        let e = BotError::Unknown("something went wrong".into());
        assert_eq!(e.to_string(), "something went wrong");
    }

    #[test]
    fn bot_error_sqlite_from() {
        let e = BotError::from(rusqlite::Error::InvalidColumnIndex(999));
        match e {
            BotError::Sqlite(_) => {}
            _ => panic!("expected Sqlite variant"),
        }
    }

    #[test]
    fn bot_error_io_from() {
        let e = BotError::from(std::io::Error::new(std::io::ErrorKind::NotFound, "file missing"));
        match e {
            BotError::Io(_) => {}
            _ => panic!("expected Io variant"),
        }
    }

    #[test]
    fn bot_error_config_source_chain() {
        let e = BotError::Config("missing token".into());
        assert!(e.to_string().contains("missing token"));
    }

    #[test]
    fn bot_error_network_from() {
        let teloxide_err = teloxide::RequestError::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"));
        let e = BotError::from(teloxide_err);
        match e {
            BotError::Network(_) => {}
            _ => panic!("expected Network variant"),
        }
    }

    #[test]
    fn result_type_alias() {
        fn returns_ok() -> Result<String> {
            Ok("hello".to_string())
        }
        assert!(returns_ok().is_ok());
    }
}

