use super::kind::ErrorKind;
use super::{ProviderError, RetryPolicy};

/// 把原始 HTTP 响应归一化为 `ProviderError` 的解析器。
///
/// 设计约束：trait 放 `model-spec-core`（仅 serde，无 HTTP 类型依赖），
/// 因此 header 以 `(String, String)` 扁平化传入，status 用 `u16`。
/// 运行时实现（`OpenAiCompatParser` 及各 provider 覆写）位于 `loom-llm`。
pub trait ProviderErrorParser: Send + Sync {
    /// 解析 HTTP 响应（状态码 + header + body）为结构化错误。
    ///
    /// 调用方（`decide()`）负责：`HeaderMap → Vec<(String,String)>` 转换、
    /// `Retry-After` 头检测与 `RetryPolicy` 覆写、`user_message` 填充。
    fn parse(&self, status: u16, headers: &[(String, String)], body: &[u8]) -> ProviderError;
}

/// 根据 `ErrorKind` 生成默认 `RetryPolicy`（未检测到 `Retry-After` 头时）。
///
/// `Server` / `Overloaded` / `RateLimited` 可重试；其余 `NoRetry` 并携带
/// `UserAction` 供消费端提示用户。
pub fn default_retry_policy(kind: ErrorKind) -> RetryPolicy {
    if kind.is_retryable() {
        RetryPolicy::Retry
    } else {
        RetryPolicy::NoRetry {
            action: kind.default_user_action(),
        }
    }
}

/// 默认用户可读消息（按 `ErrorKind` 生成中文文案）。
///
/// 消费端可按需覆盖（本地化/自定义）。
pub fn default_user_message(kind: ErrorKind) -> String {
    let msg = match kind {
        ErrorKind::BadRequest => "请求参数错误，请检查请求内容",
        ErrorKind::AuthFailed => "API Key 无效或已过期，请检查凭据",
        ErrorKind::Permission => "当前账号无权限（地区限制或未开通）",
        ErrorKind::Billing => "账户余额不足，请充值后重试",
        ErrorKind::NotFound => "模型不存在或不可用，请检查模型名",
        ErrorKind::ContentFilter => "请求或输出触发内容审核，请修改内容",
        ErrorKind::RateLimited => "请求过于频繁，请稍后重试",
        ErrorKind::QuotaExhausted => "配额已耗尽，请等待重置或升级套餐",
        ErrorKind::Overloaded => "服务过载，请稍后重试",
        ErrorKind::Server => "服务端错误，请稍后重试",
        ErrorKind::RequestTooLarge => "请求体过大，请减少输入内容",
        ErrorKind::Unknown => "未知错误，请稍后重试",
    };
    msg.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::kind::UserAction;

    #[test]
    fn default_retry_policy_mapping() {
        assert!(matches!(
            default_retry_policy(ErrorKind::Server),
            RetryPolicy::Retry
        ));
        assert!(matches!(
            default_retry_policy(ErrorKind::Overloaded),
            RetryPolicy::Retry
        ));
        assert!(matches!(
            default_retry_policy(ErrorKind::RateLimited),
            RetryPolicy::Retry
        ));
        assert!(matches!(
            default_retry_policy(ErrorKind::Billing),
            RetryPolicy::NoRetry {
                action: UserAction::TopUp
            }
        ));
        assert!(matches!(
            default_retry_policy(ErrorKind::AuthFailed),
            RetryPolicy::NoRetry {
                action: UserAction::CheckApiKey
            }
        ));
    }

    #[test]
    fn default_user_message_covered_for_all_kinds() {
        let kinds = [
            ErrorKind::BadRequest,
            ErrorKind::AuthFailed,
            ErrorKind::Permission,
            ErrorKind::Billing,
            ErrorKind::NotFound,
            ErrorKind::ContentFilter,
            ErrorKind::RateLimited,
            ErrorKind::QuotaExhausted,
            ErrorKind::Overloaded,
            ErrorKind::Server,
            ErrorKind::RequestTooLarge,
            ErrorKind::Unknown,
        ];
        for kind in kinds {
            assert!(!default_user_message(kind).is_empty(), "kind {kind:?}");
        }
    }
}
