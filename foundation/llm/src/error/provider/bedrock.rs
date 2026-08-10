//! Amazon Bedrock（Converse API）错误解析器。
//!
//! 非 OpenAI 协议：错误以异常名返回（`x-amzn-errortype` header 或 body 的 `__type`/`message`）。

use model_spec_core::error::{
    default_retry_policy, default_user_message, ErrorKind, ProviderError, ProviderErrorParser,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct BedrockBody {
    #[serde(default)]
    message: Option<String>,
    #[serde(default, rename = "__type")]
    __type: Option<String>,
}

/// Amazon Bedrock parser.
pub struct BedrockParser {
    provider_id: String,
}

impl BedrockParser {
    pub fn new(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
        }
    }
}

fn kind_from_exception(name: &str) -> ErrorKind {
    // header 值可能带 ":http://..." 或 "#..." 后缀，截取异常名部分。
    let name = name.split('#').next().unwrap_or(name);
    let name = name.split(':').next().unwrap_or(name);
    match name {
        "ValidationException" => ErrorKind::BadRequest,
        "AccessDeniedException" => ErrorKind::Permission,
        "ResourceNotFoundException" => ErrorKind::NotFound,
        "ModelTimeoutException" => ErrorKind::Server,
        "ModelNotReadyException" => ErrorKind::BadRequest,
        "ThrottlingException" => ErrorKind::RateLimited,
        "ModelErrorException" | "InternalServerException" => ErrorKind::Server,
        "ServiceUnavailableException" => ErrorKind::Overloaded,
        _ => ErrorKind::Unknown,
    }
}

impl ProviderErrorParser for BedrockParser {
    fn parse(&self, status: u16, headers: &[(String, String)], body: &[u8]) -> ProviderError {
        let mut kind = kind_from_status(status);
        let mut message = String::new();

        // 异常名优先来自 x-amzn-errortype header。
        if let Some((_, v)) = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("x-amzn-errortype"))
        {
            kind = kind_from_exception(v);
        } else if let Ok(resp) = serde_json::from_slice::<BedrockBody>(body) {
            if let Some(t) = resp.__type {
                kind = kind_from_exception(&t);
            }
            message = resp.message.unwrap_or_default();
        } else {
            message = String::from_utf8_lossy(body).into_owned();
        }

        let retry_policy = default_retry_policy(kind);

        ProviderError {
            provider_id: self.provider_id.clone(),
            kind,
            status,
            code: None,
            message,
            user_message: default_user_message(kind),
            retry_policy,
            request_id: None,
            partial_tokens: false,
        }
    }
}

fn kind_from_status(status: u16) -> ErrorKind {
    match status {
        400 => ErrorKind::BadRequest,
        403 => ErrorKind::Permission,
        404 => ErrorKind::NotFound,
        408 => ErrorKind::Server,
        409 => ErrorKind::BadRequest,
        429 => ErrorKind::RateLimited,
        424 => ErrorKind::Server,
        500 => ErrorKind::Server,
        503 => ErrorKind::Overloaded,
        _ => ErrorKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttling_from_header() {
        let headers = vec![(
            "x-amzn-errortype".to_string(),
            "ThrottlingException".to_string(),
        )];
        let err =
            BedrockParser::new("amazon-bedrock").parse(429, &headers, br#"{"message":"rate"}"#);
        assert_eq!(err.kind, ErrorKind::RateLimited);
        assert!(err.is_retryable());
    }

    #[test]
    fn validation_from_body_type() {
        let err = BedrockParser::new("amazon-bedrock").parse(
            400,
            &[],
            br#"{"__type":"ValidationException","message":"bad"}"#,
        );
        assert_eq!(err.kind, ErrorKind::BadRequest);
    }
}
