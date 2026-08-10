//! Zhipu AI / Z.AI 业务错误码覆写（open.bigmodel.cn / api.z.ai）。
//!
//! 响应体 `{"error":{"code":"1001","message":"..."}}`，业务码优先于 HTTP 状态码。

use model_spec_core::error::{ErrorKind, ProviderError, ProviderErrorParser};

use super::parse_openai_compat;

/// Zhipu / Z.AI business-code override.
pub struct ZhipuParser {
    provider_id: String,
}

impl ZhipuParser {
    pub fn new(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
        }
    }
}

impl ProviderErrorParser for ZhipuParser {
    fn parse(&self, status: u16, _headers: &[(String, String)], body: &[u8]) -> ProviderError {
        let mut err = parse_openai_compat(&self.provider_id, status, body);
        if let Some(code) = &err.code {
            let new_kind = match code.as_str() {
                "1000" | "1001" | "1003" | "1005" => ErrorKind::AuthFailed,
                "1113" => ErrorKind::Billing,
                "1200" => ErrorKind::Server,
                "1210" | "1211" | "1212" | "1213" | "1214" | "1215" => ErrorKind::BadRequest,
                "1302" | "1305" => ErrorKind::RateLimited,
                "1308" | "1309" | "1310" | "1311" => ErrorKind::QuotaExhausted,
                _ => err.kind,
            };
            if new_kind != err.kind {
                err.kind = new_kind;
                err.retry_policy = model_spec_core::error::default_retry_policy(new_kind);
                err.user_message = model_spec_core::error::default_user_message(new_kind);
            }
        }
        err
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(status: u16, body: &[u8]) -> ProviderError {
        ZhipuParser::new("zhipuai").parse(status, &[], body)
    }

    #[test]
    fn auth_business_codes() {
        let err = parse(
            401,
            br#"{"error":{"code":"1003","message":"token expired"}}"#,
        );
        assert_eq!(err.kind, ErrorKind::AuthFailed);
    }

    #[test]
    fn billing_1113() {
        let err = parse(429, br#"{"error":{"code":"1113","message":"arrears"}}"#);
        assert_eq!(err.kind, ErrorKind::Billing);
    }

    #[test]
    fn quota_1310() {
        let err = parse(
            429,
            br#"{"error":{"code":"1310","message":"weekly limit"}}"#,
        );
        assert_eq!(err.kind, ErrorKind::QuotaExhausted);
        assert!(!err.is_retryable());
    }

    #[test]
    fn bad_request_1214() {
        let err = parse(400, br#"{"error":{"code":"1214","message":"bad field"}}"#);
        assert_eq!(err.kind, ErrorKind::BadRequest);
    }
}
