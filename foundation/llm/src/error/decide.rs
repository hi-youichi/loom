//! Unified retry decision: `decide()` turns an HTTP response into a structured
//! [`ProviderError`] with `RetryPolicy`, honoring `Retry-After`.

use model_spec_core::error::{ProviderError, ProviderErrorParser, RetryPolicy};

/// Normalizes an HTTP error response into a `ProviderError`.
///
/// Responsibilities:
/// 1. Flattens `reqwest::HeaderMap` into the I/O-free representation the parser trait expects.
/// 2. Delegates status/body classification to `parser`.
/// 3. Overrides `RetryPolicy` to `RetryAfter` when a `Retry-After` header is present
///    and the error kind is retryable.
pub fn decide(
    parser: &dyn ProviderErrorParser,
    status: u16,
    headers: &reqwest::header::HeaderMap,
    body: &[u8],
) -> ProviderError {
    let flat_headers = headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                v.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect::<Vec<_>>();

    let mut err = parser.parse(status, &flat_headers, body);
    if err.kind.is_retryable() {
        if let Some(ms) = retry_after_millis(&flat_headers) {
            err.retry_policy = RetryPolicy::RetryAfter(ms);
        }
    }
    err
}

/// Parses `Retry-After` header value into milliseconds.
///
/// Supports both the HTTP integer-seconds form and the HTTP-date form.
/// Returns `None` when the header is absent or unparsable.
fn retry_after_millis(headers: &[(String, String)]) -> Option<u64> {
    let value = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))?
        .1
        .trim();

    if let Ok(secs) = value.parse::<u64>() {
        return Some(secs.saturating_mul(1000));
    }

    let dt = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let target = dt.with_timezone(&chrono::Utc);
    let diff = (target - chrono::Utc::now()).num_seconds();
    Some(diff.max(0) as u64 * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::provider::OpenAiCompatParser;

    fn decide_with_retry_after(value: Option<&str>) -> ProviderError {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(v) = value {
            headers.insert("retry-after", v.parse().unwrap());
        }
        decide(
            &OpenAiCompatParser::new("test-provider"),
            503,
            &headers,
            br#"{"error":{"message":"overloaded","type":"overloaded_error"}}"#,
        )
    }

    #[test]
    fn no_retry_after_keeps_default_policy() {
        let err = decide_with_retry_after(None);
        assert!(matches!(err.retry_policy, RetryPolicy::Retry));
    }

    #[test]
    fn integer_retry_after_parsed_to_millis() {
        let err = decide_with_retry_after(Some("3"));
        assert_eq!(err.retry_policy, RetryPolicy::RetryAfter(3000));
    }

    #[test]
    fn http_date_retry_after_parsed_to_millis() {
        // HTTP-date 格式（RFC 2822）：future 时间 → 相对毫秒。
        let future = chrono::Utc::now() + chrono::Duration::seconds(90);
        let date_str = future.to_rfc2822();
        let err = decide_with_retry_after(Some(&date_str));
        match err.retry_policy {
            RetryPolicy::RetryAfter(ms) => {
                // 90s ± 5s 容差（测试执行时间差）
                assert!(
                    (85_000..=95_000).contains(&ms),
                    "expected ~90000ms, got {ms}"
                );
            }
            other => panic!("expected RetryAfter, got {other:?}"),
        }
    }

    #[test]
    fn past_http_date_retry_after_parses_as_zero() {
        let past = chrono::Utc::now() - chrono::Duration::seconds(60);
        let date_str = past.to_rfc2822();
        let err = decide_with_retry_after(Some(&date_str));
        match err.retry_policy {
            RetryPolicy::RetryAfter(ms) => assert_eq!(ms, 0),
            other => panic!("expected RetryAfter(0), got {other:?}"),
        }
    }

    #[test]
    fn non_retryable_kind_ignores_retry_after() {
        let parser = OpenAiCompatParser::new("test-provider");
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", "5".parse().unwrap());
        // 401 → AuthFailed（不可重试）
        let err = decide(
            &parser,
            401,
            &headers,
            br#"{"error":{"message":"bad key","type":"invalid_request_error"}}"#,
        );
        assert!(matches!(
            err.retry_policy,
            RetryPolicy::NoRetry { action: _ }
        ));
    }
}
