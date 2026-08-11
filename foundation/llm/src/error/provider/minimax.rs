//! MiniMax 覆写（minimax.io / minimaxi.com）。
//!
//! 业务错误码 1000-2049（Anthropic 兼容端点），覆写映射到 `ErrorKind`。

use model_spec_core::error::{ErrorKind, ProviderError, ProviderErrorParser};

use super::parse_openai_compat;

/// MiniMax business-code override.
pub struct MiniMaxParser {
    provider_id: String,
}

impl MiniMaxParser {
    pub fn new(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
        }
    }
}

fn kind_from_business_code(code: &str) -> Option<ErrorKind> {
    Some(match code {
        "1001" | "1033" | "1024" => ErrorKind::Server, // 超时/系统/内部
        "1002" | "1041" => ErrorKind::RateLimited,     // 频率/连接数限制
        "1004" => ErrorKind::AuthFailed,               // 未授权/Token 不匹配
        "1008" => ErrorKind::Billing,                  // 余额不足
        "1026" | "1027" => ErrorKind::ContentFilter,   // 涉敏
        "1039" | "1042" | "2013" | "2049" => ErrorKind::BadRequest, // 参数/Token/URL
        _ => return None,
    })
}

impl ProviderErrorParser for MiniMaxParser {
    fn parse(&self, status: u16, _headers: &[(String, String)], body: &[u8]) -> ProviderError {
        let mut err = parse_openai_compat(&self.provider_id, status, body);
        if let Some(code) = &err.code {
            if let Some(new_kind) = kind_from_business_code(code) {
                if new_kind != err.kind {
                    err.kind = new_kind;
                    err.retry_policy = model_spec_core::error::default_retry_policy(new_kind);
                    err.user_message = model_spec_core::error::default_user_message(new_kind);
                }
            }
        }
        err
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(status: u16, body: &[u8]) -> ProviderError {
        MiniMaxParser::new("minimax").parse(status, &[], body)
    }

    #[test]
    fn billing_1008() {
        let err = parse(403, br#"{"error":{"code":"1008","message":"balance"}}"#);
        assert_eq!(err.kind, ErrorKind::Billing);
    }

    #[test]
    fn content_filter_1026() {
        let err = parse(400, br#"{"error":{"code":"1026","message":"sensitive"}}"#);
        assert_eq!(err.kind, ErrorKind::ContentFilter);
    }

    #[test]
    fn rate_limit_1002() {
        let err = parse(429, br#"{"error":{"code":"1002","message":"freq"}}"#);
        assert_eq!(err.kind, ErrorKind::RateLimited);
        assert!(err.is_retryable());
    }
}
