//! User-message truncation and env-driven display limits for the CLI.

/// Default max length for the *user message* sent to the agent (input truncation).
pub(crate) const DEFAULT_MAX_MESSAGE_LEN: usize = 200;

/// Default max length for the *reply* (assistant output) printed to stdout. 0 means no truncation.
pub(crate) const DEFAULT_MAX_REPLY_LEN: usize = 0;

/// Truncates `s` to at most `max` chars. When truncated, appends `...` (total length = max).
/// Uses character boundaries for safe UTF-8 handling.
pub(crate) fn truncate_message(s: &str, max: usize) -> String {
    const SUFFIX: &str = "...";
    let suffix_len = 3;
    if max <= suffix_len {
        return s.chars().take(max).collect();
    }
    let content_max = max - suffix_len;
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!(
        "{}{}",
        s.chars().take(content_max).collect::<String>(),
        SUFFIX
    )
}

/// Reads max message length from `HELVE_MAX_MESSAGE_LEN`. Returns default on missing/invalid.
pub(crate) fn max_message_len() -> usize {
    std::env::var("HELVE_MAX_MESSAGE_LEN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_MESSAGE_LEN)
}

/// Generates a session-unique session ID when user does not provide one.
pub(crate) fn generate_session_id() -> String {
    format!(
        "session-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_millis()
    )
}

/// Reads max reply length from `HELVE_MAX_REPLY_LEN`. 0 means no truncation. Returns default on missing/invalid.
pub(crate) fn max_reply_len() -> usize {
    std::env::var("HELVE_MAX_REPLY_LEN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_REPLY_LEN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn default_max_message_len_is_200() {
        assert_eq!(DEFAULT_MAX_MESSAGE_LEN, 200);
    }

    #[test]
    fn truncate_message_unchanged_when_short() {
        let s = "hello";
        assert_eq!(truncate_message(s, 200), "hello");
        assert_eq!(truncate_message(s, 10), "hello");
    }

    #[test]
    fn truncate_message_unchanged_when_exact() {
        let s = "a".repeat(200);
        assert_eq!(truncate_message(&s, 200), s);
    }

    #[test]
    fn truncate_message_truncates_with_suffix() {
        let s = "a".repeat(250);
        let got = truncate_message(&s, 200);
        assert_eq!(got.len(), 200);
        assert!(got.ends_with("..."));
        assert_eq!(got.chars().count(), 200);
    }

    #[test]
    fn truncate_message_utf8_safe() {
        let s = "Hello World ".repeat(20);
        let got = truncate_message(&s, 200);
        assert_eq!(got.chars().count(), 200);
        assert!(got.ends_with("..."));
    }

    #[test]
    fn default_max_reply_len_is_zero() {
        assert_eq!(DEFAULT_MAX_REPLY_LEN, 0);
    }

    #[test]
    fn env_len_and_session_id_helpers_work() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("HELVE_MAX_MESSAGE_LEN", "321");
        std::env::set_var("HELVE_MAX_REPLY_LEN", "654");
        assert_eq!(max_message_len(), 321);
        assert_eq!(max_reply_len(), 654);
        std::env::remove_var("HELVE_MAX_MESSAGE_LEN");
        std::env::remove_var("HELVE_MAX_REPLY_LEN");

        let id = generate_session_id();
        assert!(id.starts_with("session-"));
    }
}
