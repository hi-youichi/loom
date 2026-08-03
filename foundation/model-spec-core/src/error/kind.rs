use serde::{Deserialize, Serialize};

/// 跨 provider 统一的错误分类。
///
/// 解析器把原始 HTTP 状态码 + 错误体映射到稳定语义，消费端据此做重试/提示决策。
/// `QuotaExhausted` 与 `RateLimited` 分离是刻意设计：前者重试无效（需充值/等待重置），
/// 后者重试有效。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorKind {
    /// 400/422 参数错误。
    BadRequest,
    /// 401 Key 无效/过期/平台混用（如 kimi.com vs kimi.ai）。
    AuthFailed,
    /// 403 地区限制/未订阅。
    Permission,
    /// 402/403 余额不足（智谱 1113、小米 402）。
    Billing,
    /// 404 模型/资源不存在。
    NotFound,
    /// 421/451/content_filter 内容审核。
    ContentFilter,
    /// 429 限流（可退避重试）。
    RateLimited,
    /// 429 配额/额度耗尽（智谱 1308-1311、小米 Token Plan）。
    QuotaExhausted,
    /// 503/529 服务过载。
    Overloaded,
    /// 500/502/504 服务端错误。
    Server,
    /// 413 请求体过大。
    RequestTooLarge,
    /// 无法分类。
    Unknown,
}

impl ErrorKind {
    /// 该分类是否应触发应用层重试（无 `Retry-After` 头时的默认行为）。
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ErrorKind::Server | ErrorKind::Overloaded | ErrorKind::RateLimited
        )
    }

    /// 该分类对应的默认用户动作（重试无效时提示用户）。
    pub fn default_user_action(&self) -> UserAction {
        match self {
            ErrorKind::AuthFailed => UserAction::CheckApiKey,
            ErrorKind::Permission => UserAction::CheckPermission,
            ErrorKind::Billing => UserAction::TopUp,
            ErrorKind::ContentFilter => UserAction::AdjustContent,
            ErrorKind::QuotaExhausted => UserAction::WaitQuotaReset,
            ErrorKind::NotFound => UserAction::ChangeModel,
            _ => UserAction::None,
        }
    }
}

/// 用户可执行的动作，用于不可重试错误的人类可读提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserAction {
    /// API Key 无效或已过期，请检查凭据。
    CheckApiKey,
    /// 当前账号无权限（地区限制/未开通）。
    CheckPermission,
    /// 账户余额不足，请充值后重试。
    TopUp,
    /// 请求或输出触发内容审核，请修改内容。
    AdjustContent,
    /// 周/月配额已耗尽，请等待重置或升级套餐。
    WaitQuotaReset,
    /// 模型不存在或不可用，请检查模型名。
    ChangeModel,
    /// 无需用户操作。
    None,
}
