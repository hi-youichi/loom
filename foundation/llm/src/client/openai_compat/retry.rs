//! Retry configuration and backoff calculator for OpenAI-compatible clients.
//!
//! 收敛说明（2025-08）：`is_retryable_status_for` 与 `format_api_error_body` 已被
//! [`crate::error::decide::decide()`] 和 `OpenAiCompatParser` 取代并删除；
//! 本文件仅保留退避常量与指数退避计算。

/// Example default base URL (Zhipu BigModel OpenAI-compatible API).
pub(crate) const DEFAULT_BASE_URL: &str = "https://open.bigmodel.cn/api/paas/v4";

/// Max retries for retryable 5xx (500, 502, 503, 504). Total attempts = 1 + this.
pub(crate) const COMPAT_RETRY_MAX_RETRIES: u32 = 20;
/// Initial backoff before first retry.
pub(crate) const COMPAT_RETRY_INITIAL_BACKOFF: std::time::Duration =
    std::time::Duration::from_secs(1);
/// Max backoff cap.
pub(crate) const COMPAT_RETRY_MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(16);

/// Exponential backoff duration for the given 0-based attempt index.
pub(crate) fn backoff_for_attempt(attempt: u32) -> std::time::Duration {
    let max_secs = COMPAT_RETRY_MAX_BACKOFF.as_secs_f64();
    let secs =
        (COMPAT_RETRY_INITIAL_BACKOFF.as_secs_f64() * 2_f64.powi(attempt as i32)).min(max_secs);
    std::time::Duration::from_secs_f64(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_escapes_exponentially_with_cap() {
        let b0 = backoff_for_attempt(0);
        let b1 = backoff_for_attempt(1);
        let b2 = backoff_for_attempt(2);
        assert!(b1 > b0);
        assert!(b2 > b1);
        assert!(b2 <= COMPAT_RETRY_MAX_BACKOFF);
    }
}
