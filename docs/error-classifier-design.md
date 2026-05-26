# LLM 错误分类与重试策略抽象

## 背景

当前代码中存在两套独立的错误处理逻辑：

1. **标准 OpenAI Client** (`openai/mod.rs`)：使用 `classify_openai_error_message()` 判断重试
2. **OpenAI-Compatible Client** (`openai_compat.rs`)：使用 `is_retryable_status_for()` 判断重试

两套逻辑都包含：
- HTTP 状态码重试判断
- 业务错误码解析（如 BigModel 的 `1214`、MiniMax 的 `1002`）
- 网络错误模式匹配

重复代码多，且添加新 provider 需要修改多处。

## 设计目标

1. **统一接口**：单一入口处理所有 LLM 错误
2. **可扩展**：新增 provider 只需实现 trait，无需修改核心代码
3. **可测试**：各组件独立测试
4. **向后兼容**：保持现有重试行为不变

## 架构概览

```
┌─────────────────────────────────────────────────────────┐
│                     LlmClient                           │
│                  (统一错误处理入口)                      │
└─────────────────────┬───────────────────────────────────┘
                      │
        ┌─────────────┴─────────────┐
        ▼                           ▼
┌───────────────┐         ┌───────────────┐
│HttpRetryPolicy│         │ ApiErrorParser │
│               │         │                │
│  HTTP 层重试   │         │  业务错误码解析  │
│  状态码/网络   │         │  提取 code     │
└───────────────┘         └────────────────┘
```

## Trait 定义

### HttpRetryPolicy

负责判断 HTTP 层错误是否可重试。

```rust
pub trait HttpRetryPolicy: Send + Sync {
    /// 判断 HTTP 状态码 + 响应体是否可重试
    fn is_retryable_status(&self, status: u16, error_body: &str) -> bool;

    /// 判断网络错误是否可重试
    fn is_retryable_network_error(&self, error: &str) -> bool {
        error.to_lowercase().contains("timeout")
            || error.contains("connection reset")
            || error.contains("broken pipe")
            || error.contains("unexpected eof")
    }
}
```

### ApiErrorParser

负责从 API 错误消息中提取业务错误码，并判断是否可重试。

```rust
pub trait ApiErrorParser: Send + Sync {
    /// 从错误消息中提取错误码
    fn extract_error_code(&self, message: &str) -> Option<String>;

    /// 判断错误码是否可重试
    fn is_retryable_code(&self, code: &str) -> bool;

    /// 综合判断 API 错误是否可重试
    fn classify_api_error(&self, message: &str) -> RetryDecision {
        if let Some(code) = self.extract_error_code(message) {
            if self.is_retryable_code(&code) {
                return RetryDecision::Retryable;
            }
        }
        self.classify_by_message_pattern(message)
    }

    /// 通过错误消息模式判断（备选）
    fn classify_by_message_pattern(&self, message: &str) -> RetryDecision {
        RetryDecision::NonRetryable
    }
}
```

## 内置实现

### OpenAI

标准 OpenAI API 不使用业务错误码，仅依赖 HTTP 状态码。

```rust
pub struct OpenAiRetryPolicy;

impl HttpRetryPolicy for OpenAiRetryPolicy {
    fn is_retryable_status(&self, status: u16, _body: &str) -> bool {
        matches!(status, 429 | 500..=504 | 524 | 598 | 599)
    }
}

pub struct OpenAiErrorParser;

impl ApiErrorParser for OpenAiErrorParser {
    fn extract_error_code(&self, _message: &str) -> Option<String> {
        None
    }

    fn is_retryable_code(&self, _code: &str) -> bool {
        false
    }
}
```

### BigModel (智谱)

支持业务错误码解析和中文错误消息匹配。

```rust
pub struct BigModelRetryPolicy;

impl HttpRetryPolicy for BigModelRetryPolicy {
    fn is_retryable_status(&self, status: u16, body: &str) -> bool {
        // HTTP 层：429/5xx 可重试
        if matches!(status, 429 | 500..=504 | 524 | 598 | 599) {
            return true;
        }
        // 400/422 + 业务码可重试
        if matches!(status, 400 | 422) {
            let parser = BigModelErrorParser;
            return parser.classify_api_error(body) == RetryDecision::Retryable;
        }
        false
    }
}

pub struct BigModelErrorParser;

impl ApiErrorParser for BigModelErrorParser {
    fn extract_error_code(&self, message: &str) -> Option<String> {
        // 匹配 "(code: 1214)" 或 "(code:1214)"
        let lower = message.to_lowercase();
        let start = lower.find("(code:")? + 6;
        let rest = &message[start..];
        Some(rest.split(|c: char| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect())
    }

    fn is_retryable_code(&self, code: &str) -> bool {
        matches!(code,
            "500" | "1200" | "1210" | "1213" | "1214" | "1230"
            | "1231" | "1234" | "1261" | "1302" | "1303" | "1304"
            | "1305" | "1308" | "1310" | "1312" | "1313"
        )
    }

    fn classify_by_message_pattern(&self, message: &str) -> RetryDecision {
        let msg = message.to_lowercase();
        if msg.contains("参数非法") || msg.contains("并发")
            || msg.contains("频率") || msg.contains("流量限制")
            || msg.contains("访问量过大") || msg.contains("网络错误") {
            return RetryDecision::Retryable;
        }
        RetryDecision::NonRetryable
    }
}
```

### MiniMax

```rust
pub struct MiniMaxRetryPolicy;

impl HttpRetryPolicy for MiniMaxRetryPolicy {
    fn is_retryable_status(&self, status: u16, body: &str) -> bool {
        if matches!(status, 429 | 500..=504 | 524 | 598 | 599) {
            return true;
        }
        if matches!(status, 400 | 422) {
            let parser = MiniMaxErrorParser;
            return parser.classify_api_error(body) == RetryDecision::Retryable;
        }
        false
    }
}

pub struct MiniMaxErrorParser;

impl ApiErrorParser for MiniMaxErrorParser {
    fn extract_error_code(&self, message: &str) -> Option<String> {
        // MiniMax 格式: "错误描述 (code: 1002)"
        let lower = message.to_lowercase();
        let start = lower.find("(code:")? + 6;
        let rest = &message[start..];
        Some(rest.split(|c: char| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect())
    }

    fn is_retryable_code(&self, code: &str) -> bool {
        matches!(code,
            "1000" | "1001" | "1002" | "1024" | "1033" | "1041"
            | "2045" | "2056"
        )
    }

    fn classify_by_message_pattern(&self, message: &str) -> RetryDecision {
        let msg = message.to_lowercase();
        if msg.contains("请求超时") || msg.contains("请求频率超限")
            || msg.contains("内部错误") || msg.contains("系统错误")
            || msg.contains("连接数限制") {
            return RetryDecision::Retryable;
        }
        RetryDecision::NonRetryable
    }
}
```

## 统一配置

### LlmErrorClassifier 配置结构

```rust
pub struct LlmErrorClassifierConfig {
    pub http_policy: Arc<dyn HttpRetryPolicy>,
    pub api_parser: Arc<dyn ApiErrorParser>,
}

impl Default for LlmErrorClassifierConfig {
    fn default() -> Self {
        Self {
            http_policy: Arc::new(OpenAiRetryPolicy),
            api_parser: Arc::new(OpenAiErrorParser),
        }
    }
}

pub enum ProviderType {
    OpenAI,
    BigModel,
    MiniMax,
    Custom,
}

impl ProviderType {
    pub fn default_classifier(&self) -> LlmErrorClassifierConfig {
        match self {
            ProviderType::OpenAI => LlmErrorClassifierConfig::default(),
            ProviderType::BigModel => LlmErrorClassifierConfig {
                http_policy: Arc::new(BigModelRetryPolicy),
                api_parser: Arc::new(BigModelErrorParser),
            },
            ProviderType::MiniMax => LlmErrorClassifierConfig {
                http_policy: Arc::new(MiniMaxRetryPolicy),
                api_parser: Arc::new(MiniMaxErrorParser),
            },
            ProviderType::Custom => LlmErrorClassifierConfig::default(),
        }
    }
}
```

### 简化入口

```rust
impl LlmErrorClassifierConfig {
    pub fn classify(&self, error: &LlmError) -> RetryDecision {
        match error {
            LlmError::Http { status, body } => {
                if self.http_policy.is_retryable_status(*status, body) {
                    return RetryDecision::Retryable;
                }
                self.api_parser.classify_api_error(body)
            }
            LlmError::Network { message } => {
                if self.http_policy.is_retryable_network_error(message) {
                    return RetryDecision::Retryable;
                }
                self.api_parser.classify_api_error(message)
            }
            LlmError::Stream { message } => {
                self.api_parser.classify_api_error(message)
            }
        }
    }
}
```

## 迁移计划

### Phase 1: 新建模块

```
loom/src/
├── llm/
│   ├── mod.rs
│   ├── openai/
│   ├── openai_compat/
│   └── error_classifier/
│       ├── mod.rs      # Trait 定义
│       ├── openai.rs   # OpenAI 实现
│       ├── bigmodel.rs # BigModel 实现
│       ├── minimax.rs  # MiniMax 实现
│       └── tests.rs
```

### Phase 2: LLM Client 集成

1. `ChatOpenAI` 添加 `error_config: LlmErrorClassifierConfig`
2. `ChatOpenAICompat` 添加 `error_config: LlmErrorClassifierConfig`
3. 替换现有 `classify_openai_error_message` 调用
4. 替换现有 `is_retryable_status_for` 调用

### Phase 3: 清理旧代码

1. 删除 `http_retry.rs` 中的 provider 特定函数
2. 保留通用网络错误模式（如 `looks_like_transient_http_error_message`）
3. 更新测试用例

## 测试策略

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bigmodel_code_1214_is_retryable() {
        let config = ProviderType::BigModel.default_classifier();
        let err = LlmError::Http {
            status: 400,
            body: "messages 参数非法。请检查文档。 (code: 1214)".to_string(),
        };
        assert_eq!(config.classify(&err), RetryDecision::Retryable);
    }

    #[test]
    fn bigmodel_auth_error_is_not_retryable() {
        let config = ProviderType::BigModel.default_classifier();
        let err = LlmError::Http {
            status: 401,
            body: "Authentication Token非法 (code: 1002)".to_string(),
        };
        assert_eq!(config.classify(&err), RetryDecision::NonRetryable);
    }

    #[test]
    fn openai_429_is_retryable() {
        let config = ProviderType::OpenAI.default_classifier();
        let err = LlmError::Http {
            status: 429,
            body: "Rate limit exceeded".to_string(),
        };
        assert_eq!(config.classify(&err), RetryDecision::Retryable);
    }
}
```

## 后续扩展

1. **自定义 provider**：实现 `HttpRetryPolicy` + `ApiErrorParser` trait
2. **重试配置外置**：通过配置指定 max_retries、backoff 策略
3. **错误分类细化**：`Retryable(Backoff) | NonRetryable(Abort) | NonRetryable(UserAction)`