//! 月之暗面 Moonshot / Kimi 覆写（moonshot.ai / moonshot.cn / kimi.com）。
//!
//! error.type 词汇已在默认解析器覆盖（content_filter、engine_overloaded_error、
//! exceeded_current_quota_error 等）；本覆写在认证失败时补充平台 Key 隔离提示。

use model_spec_core::error::{ErrorKind, ProviderError, ProviderErrorParser};

use super::parse_openai_compat;

/// Moonshot / Kimi override: platform key-isolation hint on auth failures.
pub struct MoonshotParser {
    provider_id: String,
}

impl MoonshotParser {
    pub fn new(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
        }
    }
}

impl ProviderErrorParser for MoonshotParser {
    fn parse(&self, status: u16, _headers: &[(String, String)], body: &[u8]) -> ProviderError {
        let mut err = parse_openai_compat(&self.provider_id, status, body);
        if err.kind == ErrorKind::AuthFailed {
            err.user_message =
                "API Key 无效或平台混用：kimi.com（中国站）与 kimi.ai（国际站）账户、余额、Key 相互独立，请检查凭据"
                    .to_string();
        }
        err
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_failure_hints_key_isolation() {
        let err = MoonshotParser::new("moonshotai").parse(
            401,
            &[],
            br#"{"error":{"type":"incorrect_api_key_error","message":"bad key"}}"#,
        );
        assert_eq!(err.kind, ErrorKind::AuthFailed);
        assert!(err.user_message.contains("kimi.com"));
    }

    #[test]
    fn overloaded_maps_to_overloaded() {
        let err = MoonshotParser::new("moonshotai").parse(
            429,
            &[],
            br#"{"error":{"type":"engine_overloaded_error","message":"busy"}}"#,
        );
        assert_eq!(err.kind, ErrorKind::Overloaded);
        assert!(err.is_retryable());
    }
}
