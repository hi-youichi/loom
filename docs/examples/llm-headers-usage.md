# LLM HTTP Headers 使用示例

## 基本用法

### 1. 直接配置 Headers

```rust
use loom::llm::{ChatOpenAICompat, LlmHeaders};

let headers = LlmHeaders::default()
    .with_thread_id("thread-12345")
    .with_trace_id("trace-67890");

let client = ChatOpenAICompat::new("gpt-4")
    .unwrap()
    .with_headers(headers);

// 注意：X-App-Id 会自动设置为 "loom"
```

### 2. 使用环境变量

设置环境变量：
```bash
export LLM_THREAD_ID="thread-12345"
export LLM_TRACE_ID="trace-67890"
# 注意：不再需要设置 LLM_APP_ID，X-App-Id 会自动设置为 "loom"
```

然后在代码中使用：
```rust
use loom::llm::{ChatOpenAICompat, get_headers_from_env};

let headers = get_headers_from_env();
let client = ChatOpenAICompat::new("gpt-4")
    .unwrap()
    .with_headers(headers);
```

### 3. 添加自定义 Headers

```rust
use loom::llm::{ChatOpenAICompat, LlmHeaders};

let headers = LlmHeaders::default()
    .add_header("X-Custom-Header", "custom-value")
    .add_header("X-Request-ID", "req-123");

let client = ChatOpenAICompat::with_config(
    "https://api.openai.com/v1",
    "your-api-key",
    "gpt-4"
).with_headers(headers);

// X-App-Id 会自动设置为 "loom"
```

### 4. 链式调用

```rust
use loom::llm::{ChatOpenAICompat, LlmHeaders, ToolChoiceMode};

let client = ChatOpenAICompat::new("gpt-4")
    .unwrap()
    .with_headers(
        LlmHeaders::default()
            .with_thread_id("thread-123")
            .with_trace_id("trace-456")
    )
    .with_temperature(0.7)
    .with_tool_choice(ToolChoiceMode::Auto);

// X-App-Id 会自动设置为 "loom"
```

## ChatOpenAI 使用示例

```rust
use loom::llm::{ChatOpenAI, LlmHeaders};
use async_openai::config::OpenAIConfig;

let config = OpenAIConfig::new().with_api_key("your-api-key");

let headers = LlmHeaders::default()
    .with_thread_id("thread-12345")
    .with_trace_id("trace-67890");

let client = ChatOpenAI::with_config(config, "gpt-4")
    .with_headers(headers);

// X-App-Id 会自动设置为 "loom"
```

## 实际应用场景

### 1. 请求追踪

```rust
use loom::llm::{ChatOpenAICompat, LlmHeaders};
use uuid::Uuid;

// 为每个请求生成唯一的 trace ID
let trace_id = Uuid::new_v4().to_string();

let headers = LlmHeaders::default()
    .with_thread_id("session-123")
    .with_trace_id(&trace_id);

let client = ChatOpenAICompat::new("gpt-4")
    .unwrap()
    .with_headers(headers);

// 使用 client 进行请求...
```

### 2. 多租户应用

```rust
use loom::llm::{ChatOpenAICompat, LlmHeaders};

fn create_client_for_tenant(tenant_id: &str, user_id: &str) -> ChatOpenAICompat {
    let headers = LlmHeaders::default()
        .with_thread_id(&format!("tenant-{}-user-{}", tenant_id, user_id))
        .add_header("X-Tenant-ID", tenant_id)
        .add_header("X-User-ID", user_id);

    ChatOpenAICompat::new("gpt-4")
        .unwrap()
        .with_headers(headers)
        // X-App-Id 会自动设置为 "loom"
}

// 使用示例
let client = create_client_for_tenant("tenant-123", "user-456");
```

### 3. A/B 测试

```rust
use loom::llm::{ChatOpenAICompat, LlmHeaders};

fn create_client_with_experiment(experiment_group: &str) -> ChatOpenAICompat {
    let headers = LlmHeaders::default()
        .add_header("X-Experiment-Group", experiment_group)
        .add_header("X-Experiment-ID", "exp-model-selection");

    ChatOpenAICompat::new("gpt-4")
        .unwrap()
        .with_headers(headers)
        // X-App-Id 会自动设置为 "loom"
}

// A/B 测试
let client_a = create_client_with_experiment("control");
let client_b = create_client_with_experiment("treatment");
```

## 监控和日志集成

### 1. 与分布式追踪系统集成

```rust
use loom::llm::{ChatOpenAICompat, LlmHeaders};

fn create_client_with_tracing(tracer: &dyn Tracer) -> ChatOpenAICompat {
    let span = tracer.start_span("llm_client_creation");
    
    let headers = LlmHeaders::default()
        .with_thread_id(&span.span_id())
        .with_trace_id(&span.trace_id())
        .add_header("X-Sampled", "1");

    span.end();
    
    ChatOpenAICompat::new("gpt-4")
        .unwrap()
        .with_headers(headers)
        // X-App-Id 会自动设置为 "loom"
}
```

### 2. 错误追踪

```rust
use loom::llm::{ChatOpenAICompat, LlmHeaders};

fn create_client_with_error_tracking() -> ChatOpenAICompat {
    let headers = LlmHeaders::default()
        .add_header("X-Error-Tracking-Enabled", "true")
        .add_header("X-Service-Version", env!("CARGO_PKG_VERSION"));

    ChatOpenAICompat::new("gpt-4")
        .unwrap()
        .with_headers(headers)
        // X-App-Id 会自动设置为 "loom"
}
```

## 重要变更说明

### X-App-Id 固定为 "loom"

从现在开始，`X-App-Id` HTTP header 固定设置为 `"loom"`，无需用户配置：

**之前的用法**：
```rust
let headers = LlmHeaders::default()
    .with_app_id("my-app-id")  // 不再需要
    .with_thread_id("thread-123");
```

**现在的用法**：
```rust
let headers = LlmHeaders::default()
    .with_thread_id("thread-123")
    .with_trace_id("trace-456");
// X-App-Id 会自动设置为 "loom"
```

**环境变量变更**：
```bash
# 之前
export LLM_APP_ID="my-app-id"      # 不再需要
export LLM_THREAD_ID="thread-123"
export LLM_TRACE_ID="trace-456"

# 现在
export LLM_THREAD_ID="thread-123"  # 只需要这两个
export LLM_TRACE_ID="trace-456"
```

## 注意事项

1. **Header 大小限制**：HTTP headers 有大小限制，避免添加过大的值
2. **敏感信息**：不要在 headers 中包含敏感信息如 API 密钥
3. **性能影响**：每个请求会额外传输约 100-200 字节的 header 数据
4. **兼容性**：X-App-Id 固定功能完全向后兼容，现有代码无需修改
5. **多租户场景**：如需区分不同租户，可以使用自定义 headers

## 完整示例

```rust
use loom::llm::{ChatOpenAICompat, LlmHeaders, ToolChoiceMode};
use loom::message::{Message, UserContent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 配置 headers（X-App-Id 会自动设置为 "loom"）
    let headers = LlmHeaders::default()
        .with_thread_id("user-session-12345")
        .with_trace_id("request-67890")
        .add_header("X-Environment", "production")
        .add_header("X-Region", "us-west-2");

    // 创建客户端
    let client = ChatOpenAICompat::new("gpt-4")
        .unwrap()
        .with_headers(headers)
        .with_temperature(0.7)
        .with_tool_choice(ToolChoiceMode::Auto);

    // 准备消息
    let messages = vec![
        Message::System("You are a helpful assistant.".to_string()),
        Message::User(UserContent::Text("Hello, how are you?".to_string())),
    ];

    // 发送请求（请求会自动包含配置的 headers 和 X-App-Id: loom）
    let response = client.invoke(&messages).await?;
    
    println!("Response: {}", response.content);
    
    Ok(())
}
```

## 相关文档

- [LLM HTTP Headers 技术方案](../plans/llm-http-headers.md)
- [LLM 集成指南](../guides/llm-integration.md)