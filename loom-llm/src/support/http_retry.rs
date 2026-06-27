//! Re-export of HTTP retry utilities from `loom-http-retry`.
//!
//! Kept as a module so existing `loom_llm::support::http_retry::*` imports
//! (both inside loom-llm and in downstream crates) continue to work unchanged.

pub use loom_http_retry::{
    is_retryable_reqwest_error, looks_like_transient_http_error_message,
    retry_backoff_for_attempt, TRANSIENT_HTTP_INITIAL_BACKOFF, TRANSIENT_HTTP_MAX_BACKOFF,
    TRANSIENT_HTTP_MAX_RETRIES,
};
