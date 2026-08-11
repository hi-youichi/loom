//! 阶跃星辰 StepFun 覆写（stepfun.com / stepfun.ai）。
//!
//! 特有 HTTP **451 内容审核未通过** → ContentFilter。

use model_spec_core::error::{ErrorKind, ProviderError, ProviderErrorParser};

use super::parse_openai_compat;

/// StepFun override: HTTP 451 → ContentFilter.
pub struct StepFunParser {
    provider_id: String,
}

impl StepFunParser {
    pub fn new(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
        }
    }
}

impl ProviderErrorParser for StepFunParser {
    fn parse(&self, status: u16, _headers: &[(String, String)], body: &[u8]) -> ProviderError {
        let mut err = parse_openai_compat(&self.provider_id, status, body);
        if status == 451 {
            err.kind = ErrorKind::ContentFilter;
            err.retry_policy = model_spec_core::error::default_retry_policy(err.kind);
            err.user_message = model_spec_core::error::default_user_message(err.kind);
        }
        err
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_451_to_content_filter() {
        let err =
            StepFunParser::new("stepfun").parse(451, &[], br#"{"error":{"message":"blocked"}}"#);
        assert_eq!(err.kind, ErrorKind::ContentFilter);
        assert!(!err.is_retryable());
    }
}
