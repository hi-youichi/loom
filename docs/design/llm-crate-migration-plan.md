# LLM 模块独立为 crate 方案

## 目标

将 LLM 相关代码独立为 `loom-llm` crate，移除循环依赖，实现清晰的模块边界。

## 最终结构

```
loom-llm/                      # LLM 独立 crate
├── Cargo.toml
└── src/
    ├── lib.rs                 # 入口
    ├── message.rs             # Message, UserContent, ContentPart (从 loom 迁移)
    ├── tool.rs                # ToolCall, ToolSpec (新建)
    ├── error.rs               # LlmError (扩展现有)
    ├── traits.rs              # LlmClient, LlmProvider, LlmResponse
    ├── client/
    │   └── openai_compat.rs   # ChatOpenAICompat
    ├── audit.rs, thinking.rs, error_classifier/
    └── ...

loom/                         # Agent 运行时
├── Cargo.toml
└── src/
    ├── llm/
    │   └── mod.rs             # 简化为 re-export 层 + 保留实现
    ├── state/
    │   └── react_state.rs      # 使用 loom_llm::ToolCall
    └── ...
```

## 类型归属

| 类型 | 归属 | 说明 |
|------|------|------|
| `Message`, `UserContent`, `ContentPart` | loom-llm | LLM 输入输出格式 |
| `ToolCall` | loom-llm | LLM 生成的工具调用 |
| `ToolSpec` | loom-llm | 工具定义 |
| `LlmClient`, `LlmProvider` | loom-llm | LLM 抽象接口 |
| `LlmResponse` | loom-llm | LLM 响应 |
| `LlmError` | loom-llm | LLM 错误 |
| `ReActState` | loom | Agent 状态 |
| `ToolResult` | loom | 工具执行结果 |

## 核心类型定义

### loom-llm/src/tool.rs (新建)

```rust
//! Tool types for LLM function calling.

use serde::{Deserialize, Serialize};

/// A tool call produced by the LLM (ThinkNode output, ActNode input).
///
/// Aligned with OpenAI `tool_calls` format.
/// The `id` field is optional for backward compatibility.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    /// Unique identifier for correlating with ToolResult.
    pub id: Option<String>,
    /// Tool name as registered in ToolSource.
    pub name: String,
    /// Arguments as JSON string; parsed in Act when calling the tool.
    pub arguments: String,
}

impl ToolCall {
    pub fn new(name: impl Into<String>, arguments: impl Into<String>) -> Self {
        Self {
            id: None,
            name: name.into(),
            arguments: arguments.into(),
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

/// Tool specification advertised to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolSpec {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }
}
```

### loom-llm/src/message.rs (从 loom 迁移)

从 `loom/src/message.rs` 迁移以下类型：
- `UserContent`
- `ContentPart`
- `ContentError`
- `AssistantToolCall`
- `AssistantPayload`
- `Message`

### loom-llm/src/error.rs (扩展)

扩展现有错误类型，添加与 `AgentError` 的转换：

```rust
use thiserror::Error;

/// LLM execution error.
#[derive(Debug, Error)]
pub enum LlmError {
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
    #[error("run cancelled")]
    Cancelled,
    #[error("empty response after {retries} retries")]
    EmptyResponse { retries: u32 },
    #[error("timeout")]
    Timeout,
    #[error("api error: {code} - {message}")]
    ApiError { code: String, message: String },
    // ... 其他
}

/// Conversion from loom::AgentError for compatibility.
impl From<crate::AgentError> for LlmError {
    fn from(e: crate::AgentError) -> Self {
        match e {
            AgentError::ExecutionFailed(s) => LlmError::ExecutionFailed(s),
            AgentError::Cancelled => LlmError::Cancelled,
            AgentError::Interrupted(_) => LlmError::ExecutionFailed("interrupted".into()),
            AgentError::EmptyLlmResponse { retries } => LlmError::EmptyResponse { retries },
        }
    }
}
```

### loom-llm/src/traits.rs (更新)

```rust
use crate::message::Message;
use crate::tool::ToolCall;
use crate::error::LlmError;
use async_trait::async_trait;
use tokio::sync::mpsc;

/// Response from an LLM completion.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub reasoning_content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<LlmUsage>,
}

/// LLM client trait.
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, LlmError>;
    async fn invoke_stream(&self, messages: &[Message], chunk_tx: Option<mpsc::Sender<MessageChunk>>) -> Result<LlmResponse, LlmError>;
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError>;
}
```

## loom/src/llm/mod.rs (简化)

```rust
//! LLM module - re-exports from loom-llm with backward compatibility.

pub use loom_llm::message::{
    Message, UserContent, ContentPart, ContentError,
    AssistantPayload, AssistantToolCall, ToolCallContent,
    assistant_content_for_chat_api,
};
pub use loom_llm::tool::{ToolCall, ToolSpec};
pub use loom_llm::error::LlmError;
pub use loom_llm::traits::{
    LlmClient, LlmResponse, LlmUsage, LlmHeaders, ToolChoiceMode,
    ToolCallDelta, ModelInfo, LlmProvider, PromptTokensDetails, CompletionTokensDetails,
    MessageChunk, ProviderConfig,
};

use crate::error::AgentError;

// Error conversion for loom compatibility
impl From<LlmError> for AgentError {
    fn from(e: LlmError) -> Self {
        match e {
            LlmError::ExecutionFailed(s) => AgentError::ExecutionFailed(s),
            LlmError::Cancelled => AgentError::Cancelled,
            LlmError::EmptyResponse { retries } => AgentError::EmptyLlmResponse { retries },
            _ => AgentError::ExecutionFailed(e.to_string()),
        }
    }
}

// ============================================================================
// Loom-specific LLM implementations
// ============================================================================

mod openai;
mod openai_compat;
mod openai_provider;
mod openai_compat_provider;
mod fixed_provider;
mod mock;
mod factory;
mod model_cache;
mod model_registry;
mod retry;
mod error_classifier;
pub mod audit;

pub use openai::ChatOpenAI;
pub use openai_compat::ChatOpenAICompat;
pub use openai_provider::OpenAIProvider;
pub use openai_compat_provider::ChatOpenAICompat;
pub use fixed_provider::FixedLlmProvider;
pub use mock::{MockLlm, MultiRoundMockLlm};
pub use factory::LlmFactory;
pub use model_cache::{fetch_provider_models, ModelCache};
pub use model_registry::{create_llm_provider, create_llm_client, ModelEntry};
pub use retry::RetryLlmClient;

pub use error_classifier::{
    HttpRetryPolicy, ApiErrorParser, LlmErrorClassifierConfig, ProviderType, RetryDecision,
};

pub use audit::{
    build_audit_entry, FileLlmAuditLog, LlmAuditConfig, LlmAuditEntry, LlmAuditLog,
    LlmAuditRequest, LlmAuditRequestParams, LlmAuditResponse, LlmAuditToolCall,
    LlmAuditUsage, NoOpLlmAuditLog,
};

pub use loom_llm::thinking::{collect_thinking_tags, strip_thinking_tags};

/// Load LLM headers from environment variables.
pub fn get_headers_from_env() -> LlmHeaders {
    LlmHeaders {
        thread_id: std::env::var("LLM_THREAD_ID").ok(),
        trace_id: std::env::var("LLM_TRACE_ID").ok(),
        custom_headers: std::collections::HashMap::new(),
    }
}
```

## loom/src/state/react_state.rs (更新)

```rust
use loom_llm::message::Message;
use loom_llm::tool::ToolCall;  // 使用 LLM 中的 ToolCall
// import ToolResult 保持在 state 中（工具执行结果不是 LLM 输出）

pub struct ReActState {
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCall>,  // 改为 loom_llm::ToolCall
    pub tool_results: Vec<ToolResult>,
    // ...
}

// 转换函数：AssistantToolCall -> ToolCall
impl From<AssistantToolCall> for ToolCall {
    fn from(tc: AssistantToolCall) -> Self {
        Self {
            id: Some(tc.id),
            name: tc.name,
            arguments: tc.arguments,
        }
    }
}
```

## 迁移文件清单

### 新建

| 文件 | 说明 |
|------|------|
| `loom-llm/src/tool.rs` | ToolCall, ToolSpec 定义 |

### 迁移 (loom → loom-llm)

| 文件 | 从 | 到 |
|------|------|------|
| `message.rs` | `loom/src/message.rs` | `loom-llm/src/message.rs` |

### 修改

| 文件 | 变更 |
|------|------|
| `loom-llm/src/lib.rs` | 添加 re-exports |
| `loom-llm/src/error.rs` | 扩展错误，添加 From<AgentError> |
| `loom-llm/src/traits.rs` | 使用统一的 ToolCall |
| `loom/src/llm/mod.rs` | 简化为 re-export 层 |
| `loom/src/state/react_state.rs` | 使用 loom_llm::ToolCall |
| `loom/src/llm/openai_compat.rs` | 更新导入 |
| `loom/src/llm/openai/mod.rs` | 更新导入 |
| `loom/src/llm/mock.rs` | 更新导入 |
| `loom/src/llm/tool_call_accumulator.rs` | 更新导入 |
| `loom/src/agent/tot/expand_node.rs` | 更新导入 |
| `loom/src/agent/react/mod.rs` | 更新导入 |
| `loom/src/agent/got/state.rs` | 更新导入 |
| `loom/src/agent/tot/runner.rs` | 更新导入 |
| `loom/src/agent/got/runner.rs` | 更新导入 |
| `loom/src/agent/dup/runner.rs` | 更新导入 |

## 实施步骤

### Phase 1: 创建 tool.rs (0.5 天)

```bash
# 1. 创建 loom-llm/src/tool.rs
# 2. 定义 ToolCall, ToolSpec
# 3. 验证编译
```

### Phase 2: 迁移 message.rs (0.5 天)

```bash
# 1. 复制 loom/src/message.rs → loom-llm/src/message.rs
# 2. 移除 loom-specific 依赖 (如 uuid6)
# 3. 添加 From<AssistantToolCall> for ToolCall
# 4. 验证编译
```

### Phase 3: 更新 traits.rs (0.5 天)

```bash
# 1. 更新 LlmResponse.tool_calls: Vec<ToolCall>
# 2. 更新 LlmClient::invoke 返回 LlmError
# 3. 验证编译
```

### Phase 4: 更新 loom (1 天)

```bash
# 1. 更新 loom/src/llm/mod.rs (re-export)
# 2. 更新 state/react_state.rs
# 3. 更新所有使用 ToolCall 的文件
# 4. 验证编译
```

### Phase 5: 测试验证 (0.5 天)

```bash
cargo check -p loom-llm
cargo check -p loom
cargo test -p loom
```

## 关键变更总结

| 变更前 | 变更后 |
|--------|--------|
| `use crate::state::ToolCall` | `use loom_llm::ToolCall` |
| `LlmClient::invoke -> Result<..., AgentError>` | `LlmClient::invoke -> Result<..., LlmError>` |
| ToolCall 定义在 state/react_state.rs | ToolCall 定义在 loom-llm/tool.rs |
| Message 定义在 loom/src/message.rs | Message 定义在 loom-llm/message.rs |

## 验证清单

- [ ] `cargo check -p loom-llm`
- [ ] `cargo check -p loom`
- [ ] `cargo test -p loom-llm`
- [ ] `cargo test -p loom`
- [ ] 所有 14 个文件更新完成
- [ ] 序列化/反序列化测试通过

## 时间估算

| Phase | 任务 | 时间 |
|-------|------|------|
| 1 | 创建 tool.rs | 0.5 天 |
| 2 | 迁移 message.rs | 0.5 天 |
| 3 | 更新 traits.rs | 0.5 天 |
| 4 | 更新 loom 及所有引用 | 1 天 |
| 5 | 测试验证 | 0.5 天 |
| **总计** | | **3 天** |