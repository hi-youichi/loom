//! 美团 LongCat 覆写（longcat.chat）。
//!
//! 失败响应不计费（仅 HTTP 200 计费）；余额类错误时补充该提示。

use model_spec_core::error::{ErrorKind, ProviderError, ProviderErrorParser};

use super::parse_openai_compat;

/// LongCat override: billing hint (failure responses are not charged).
pub struct LongCatParser {
    provider_id: String,
}

impl LongCatParser {
    pub fn new(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
        }
    }
}

impl ProviderErrorParser for LongCatParser {
    fn parse(&self, status: u16, _headers: &[(String, String)], body: &[u8]) -> ProviderError {
        let mut err = parse_openai_compat(&self.provider_id, status, body);
        if matches!(err.kind, ErrorKind::Billing | ErrorKind::QuotaExhausted) {
            err.user_message = format!(
                "{}（LongCat 仅 HTTP 200 按实际 Token 计费，失败响应不计费）",
                err.user_message
            );
        }
        err
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn billing_hints_free_failures() {
        let err = LongCatParser::new("longcat").parse(
            402,
            &[],
            br#"{"error":{"message":"token plan exhausted","code":"insufficient_quota"}}"#,
        );
        assert_eq!(err.kind, ErrorKind::Billing);
        assert!(err.user_message.contains("不计费"));
    }
}
