//! Azure OpenAI 错误解析器。
//!
//! Azure 使用与 OpenAI 相同的错误协议（error schema + HTTP 状态码），附加差异：
//! deployment 不存在返回 404/403、内容过滤命中返回 400 + `code: content_filter`
//! （后者已由默认解析器的 error.type/code 词汇覆盖）。因此直接复用 OpenAI 兼容解析。

use model_spec_core::error::{ProviderError, ProviderErrorParser};

use super::parse_openai_compat;

/// Azure OpenAI parser（复用 OpenAI 协议；差异点由默认词汇覆盖）。
pub struct AzureParser {
    provider_id: String,
}

impl AzureParser {
    pub fn new(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
        }
    }
}

impl ProviderErrorParser for AzureParser {
    fn parse(&self, status: u16, _headers: &[(String, String)], body: &[u8]) -> ProviderError {
        parse_openai_compat(&self.provider_id, status, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_filter_code() {
        let err = AzureParser::new("azure").parse(
            400,
            &[],
            br#"{"error":{"message":"filtered","code":"content_filter","type":null}}"#,
        );
        assert_eq!(err.kind, model_spec_core::error::ErrorKind::ContentFilter);
    }
}
