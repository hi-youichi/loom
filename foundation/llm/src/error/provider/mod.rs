//! Provider-specific error parsers.
//!
//! Each parser normalizes raw HTTP responses into [`model_spec_core::error::ProviderError`].
//! The default is [`OpenAiCompatParser`] (covers ~146 OpenAI-compatible providers);
//! overrides handle provider-specific business codes, special HTTP statuses and
//! non-OpenAI protocols (Anthropic / Google / Bedrock).

pub mod anthropic;
pub mod azure;
pub mod bedrock;
pub mod google;
pub mod longcat;
pub mod minimax;
pub mod moonshot;
pub mod openai;
pub mod openrouter;
pub mod registry;
pub mod stepfun;
pub mod xiaomi;
pub mod zhipu;

pub use anthropic::AnthropicParser;
pub use azure::AzureParser;
pub use bedrock::BedrockParser;
pub use google::GoogleParser;
pub use longcat::LongCatParser;
pub use minimax::MiniMaxParser;
pub use moonshot::MoonshotParser;
pub use openai::OpenAiCompatParser;
pub use openrouter::OpenRouterParser;
pub use registry::parser_for;
pub use stepfun::StepFunParser;
pub use xiaomi::XiaomiParser;
pub use zhipu::ZhipuParser;

use model_spec_core::error::{ErrorKind, ProviderError};
use serde::Deserialize;

#[derive(Deserialize)]
struct ApiErrorDetail {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

#[derive(Deserialize)]
struct ApiErrorResponse {
    #[serde(default)]
    error: Option<ApiErrorDetail>,
    #[serde(default)]
    request_id: Option<String>,
}

/// 按 OpenAI 协议解析错误体，产出基础 `ProviderError`（含默认 retry_policy / user_message）。
///
/// 覆写解析器先调用本函数，再按 provider 特有业务码/状态码修正 `kind`。
pub(crate) fn parse_openai_compat(provider_id: &str, status: u16, body: &[u8]) -> ProviderError {
    let mut kind = kind_from_status(status);
    let mut code = None;
    let mut message = String::new();
    let mut request_id = None;

    if let Ok(resp) = serde_json::from_slice::<ApiErrorResponse>(body) {
        if let Some(detail) = resp.error {
            message = detail.message.unwrap_or_default();
            if let Some(k) = detail.r#type.as_deref().and_then(kind_from_error_type) {
                kind = k;
            } else if let Some(c) = &detail.code {
                // 部分 provider 只在 code 里给词汇（如 Azure content_filter）。
                if let Some(k) = kind_from_error_code(c) {
                    kind = k;
                }
            }
            code = detail.code;
        }
        request_id = resp.request_id;
    }

    if status == 429 && kind == ErrorKind::RateLimited && is_quota_429(&message, code.as_deref()) {
        kind = ErrorKind::QuotaExhausted;
    }

    finish(provider_id, kind, status, code, message, request_id)
}

/// 组装 `ProviderError`（默认 retry_policy + user_message）。
pub(crate) fn finish(
    provider_id: &str,
    kind: ErrorKind,
    status: u16,
    code: Option<String>,
    message: String,
    request_id: Option<String>,
) -> ProviderError {
    let retry_policy = model_spec_core::error::default_retry_policy(kind);

    ProviderError {
        provider_id: provider_id.to_string(),
        kind,
        status,
        code,
        message,
        user_message: model_spec_core::error::default_user_message(kind),
        retry_policy,
        request_id,
        partial_tokens: false,
    }
}

/// Maps HTTP status code to a base `ErrorKind`（OpenAI 系）。
pub(crate) fn kind_from_status(status: u16) -> ErrorKind {
    match status {
        400 | 422 => ErrorKind::BadRequest,
        401 => ErrorKind::AuthFailed,
        402 => ErrorKind::Billing,
        403 => ErrorKind::Permission,
        404 => ErrorKind::NotFound,
        413 => ErrorKind::RequestTooLarge,
        429 => ErrorKind::RateLimited,
        500 | 502 | 504 => ErrorKind::Server,
        503 | 529 => ErrorKind::Overloaded,
        _ => ErrorKind::Unknown,
    }
}

/// Maps `error.type` vocabulary to a more specific `ErrorKind`（OpenAI 系）。
pub(crate) fn kind_from_error_type(ty: &str) -> Option<ErrorKind> {
    match ty {
        "content_filter" | "content_filter_error" => Some(ErrorKind::ContentFilter),
        "invalid_authentication_error" | "incorrect_api_key_error" => Some(ErrorKind::AuthFailed),
        "engine_overloaded_error" | "overloaded_error" => Some(ErrorKind::Overloaded),
        "exceeded_current_quota_error" | "insufficient_quota" | "credit_balance_exhausted" => {
            Some(ErrorKind::Billing)
        }
        "rate_limit_reached_error" | "rate_limit_error" => Some(ErrorKind::RateLimited),
        "request_too_large" => Some(ErrorKind::RequestTooLarge),
        "timeout_error" => Some(ErrorKind::Server),
        "api_error" | "server_error" => Some(ErrorKind::Server),
        _ => None,
    }
}

/// Maps `error.code` vocabulary to a more specific `ErrorKind`（当 `error.type` 缺失时）。
pub(crate) fn kind_from_error_code(code: &str) -> Option<ErrorKind> {
    match code {
        "content_filter" | "content_filter_error" => Some(ErrorKind::ContentFilter),
        "insufficient_quota" | "credit_balance_exhausted" => Some(ErrorKind::Billing),
        _ => None,
    }
}

/// Heuristic for 429 bodies that indicate quota exhaustion rather than rate limiting.
pub(crate) fn is_quota_429(message: &str, code: Option<&str>) -> bool {
    // 智谱业务码：1302/1305 为速率限制（可退避重试），1308-1311 为使用量/套餐/限额耗尽。
    if let Some(c) = code {
        match c {
            "1308" | "1309" | "1310" | "1311" => return true,
            "1302" | "1305" => return false,
            _ => {}
        }
    }
    let haystack = format!("{message} {}", code.unwrap_or_default()).to_lowercase();
    [
        "quota",
        "credit",
        "balance",
        "insufficient",
        "spend limit",
        "usage limit",
    ]
    .iter()
    .any(|kw| haystack.contains(kw))
}
