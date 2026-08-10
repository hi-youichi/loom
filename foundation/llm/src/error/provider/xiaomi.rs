//! 小米 MiMo 覆写（xiaomimimo.com）。
//!
//! 特有 HTTP **421 内容拦截** → ContentFilter；其余走 OpenAI 协议。

use model_spec_core::error::{ErrorKind, ProviderError, ProviderErrorParser};

use super::parse_openai_compat;

/// Xiaomi MiMo override: HTTP 421 → ContentFilter.
pub struct XiaomiParser {
    provider_id: String,
}

impl XiaomiParser {
    pub fn new(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
        }
    }
}

impl ProviderErrorParser for XiaomiParser {
    fn parse(&self, status: u16, headers: &[(String, String)], body: &[u8]) -> ProviderError {
        let mut err = parse_openai_compat(&self.provider_id, status, body);
        if status == 421 {
            err.kind = ErrorKind::ContentFilter;
            err.retry_policy = model_spec_core::error::default_retry_policy(err.kind);
            err.user_message = model_spec_core::error::default_user_message(err.kind);
        }
        let _ = headers;
        err
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_421_to_content_filter() {
        let err =
            XiaomiParser::new("xiaomi").parse(421, &[], br#"{"error":{"message":"blocked"}}"#);
        assert_eq!(err.kind, ErrorKind::ContentFilter);
        assert!(!err.is_retryable());
    }
}
