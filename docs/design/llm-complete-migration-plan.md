# 完全删除 loom/src/llm 方案

## 核心问题

| 问题 | loom | loom-llm |
|------|------|----------|
| `LlmClient::invoke` 返回 | `Result<LlmResponse, AgentError>` | `Result<LlmResponse, LlmError>` |
| `LlmProvider::create_client` 返回 | `Result<Box<dyn LlmClient>, AgentError>` | `Result<Box<dyn LlmClient>, LlmError>` |
| 错误类型 | `AgentError` (在 loom/src/error.rs) | `LlmError` (在 loom-llm/src/error.rs) |

## 解决方案：Thin Adapter Layer

### 最终结构

```
loom/src/
├── llm/
│   ├── mod.rs           # 简化为 re-export + 适配层
│   ├── adapter.rs       # 新建: LlmError → AgentError 适配器
│   └── openai/          # 保留: loom-specific OpenAI 实现
├── error.rs             # AgentError (保持)
└── ...

loom-llm/src/
├── lib.rs               # LlmClient, LlmResponse, Message, ToolCall
├── traits.rs            # 核心 trait (返回 LlmError)
└── ...
```

### 方案实施

#### 1. 创建 adapter.rs

```rust
//! Adapter layer between loom-llm and loom.
//!
//! Converts LlmError from loom-llm to AgentError from loom.

use crate::error::AgentError;
use loom_llm::LlmError;

impl From<LlmError> for AgentError {
    fn from(e: LlmError) -> Self {
        match e {
            LlmError::RequestFailed(s) => AgentError::ExecutionFailed(s),
            LlmError::Timeout => AgentError::ExecutionFailed("timeout".into()),
            LlmError::ConnectionError(s) => AgentError::ExecutionFailed(s),
            LlmError::EmptyResponse => AgentError::EmptyLlmResponse { retries: 0 },
            _ => AgentError::ExecutionFailed(e.to_string()),
        }
    }
}

/// Wrapper that converts loom-llm LlmClient to loom's LlmClient.
pub struct LoomLlmAdapter<C> {
    inner: C,
}

impl<C> LoomLlmAdapter<C> {
    pub fn new(inner: C) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl<C: loom_llm::LlmClient> crate::llm::LlmClient for LoomLlmAdapter<C> {
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, AgentError> {
        self.inner.invoke(messages).await.map_err(Into::into)
    }

    async fn invoke_stream(
        &self,
        messages: &[Message],
        chunk_tx: Option<tokio::sync::mpsc::Sender<crate::stream::MessageChunk>>,
    ) -> Result<LlmResponse, AgentError> {
        self.inner.invoke_stream(messages, chunk_tx).await.map_err(Into::into)
    }
}
```

#### 2. 更新 mod.rs

```rust
//! LLM module - thin wrapper around loom-llm with loom-specific adapters.

pub use loom_llm::message::{Message, UserContent, ContentPart, ContentError, AssistantToolCall, AssistantPayload, ToolCallContent, assistant_content_for_chat_api};
pub use loom_llm::tool::{ToolCall, ToolSpec};
pub use loom_llm::traits::{LlmUsage, LlmHeaders, ToolChoiceMode, ModelInfo, ModelCapabilities, PromptTokensDetails, CompletionTokensDetails};

mod adapter;
pub use adapter::LoomLlmAdapter;

mod openai;
pub use openai::ChatOpenAI;

// 导入但需要适配的类型
pub use loom_llm::traits::{
    LlmClient as LlmClientTrait,  // 改名避免冲突
    LlmProvider as LlmProviderTrait,
    LlmResponse as LlmResponseTrait,
    ToolCallDelta,
    MessageChunk,
    ProviderConfig,
    ModelEntry,
};
```

#### 3. 关键变更：创建 Loom LlmClient trait

```rust
// loom/src/llm/traits.rs (新建)

// 导入 loom-llm 的 trait 并重命名
pub use loom_llm::traits::LlmResponse as LlmResponseCore;

// loom 自己的 LlmClient (返回 AgentError)
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponseCore, AgentError>;
    async fn invoke_stream(&self, messages: &[Message], chunk_tx: Option<mpsc::Sender<MessageChunk>>) -> Result<LlmResponseCore, AgentError>;
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn create_client(&self, model: &str) -> Result<Box<dyn LlmClient>, AgentError>;
    fn create_client_with_headers(&self, model: &str, headers: Option<LlmHeaders>) -> Result<Box<dyn LlmClient>, AgentError>;
    fn default_model(&self) -> &str;
    fn provider_name(&self) -> &str;
}
```

#### 4. 批量替换所有 `impl LlmClient`

找到所有 `impl LlmClient for ...` 的地方，改为 `impl LlmClient for ...` 使用 Adapter：

```rust
// openai/mod.rs
impl loom_llm::LlmClient for ChatOpenAI {
    async fn invoke(...) -> Result<..., loom_llm::LlmError> { ... }
}

// 新增 adapter 实现
impl crate::llm::LlmClient for ChatOpenAI {
    async fn invoke(...) -> Result<..., AgentError> {
        self.invoke(messages).await.map_err(Into::into)
    }
}
```

或者更简单的方式：在每个 `impl LlmClient` 的地方添加 `map_err(Into::into)`。

---

## 文件变更清单

### 需要删除

| 文件 | 原因 |
|------|------|
| `loom/src/llm/mod.rs` | 替换为简化版 |
| `loom/src/llm/factory.rs` | 逻辑移到 adapter |
| `loom/src/llm/mock.rs` | 使用 loom-llm 的 MockLlm |
| `loom/src/llm/model_cache.rs` | 使用 loom-llm 的 ModelCache |
| `loom/src/llm/model_registry.rs` | 使用 loom-llm 的 ModelRegistry |
| `loom/src/llm/retry.rs` | 逻辑移到 adapter |
| `loom/src/llm/error_classifier/` | 使用 loom-llm 的 |
| `loom/src/llm/audit.rs` | 使用 loom-llm 的 |
| `loom/src/llm/thinking.rs` | 使用 loom-llm 的 |
| `loom/src/llm/tool_call_accumulator.rs` | 使用 loom-llm 的 |
| `loom/src/llm/compat.rs` | 不再需要 |

### 需要保留

| 文件 | 原因 |
|------|------|
| `loom/src/llm/openai/` | loom-specific OpenAI 实现 |
| `loom/src/llm/openai_compat.rs` | loom-specific 兼容实现 |
| `loom/src/llm/openai_provider.rs` | loom-specific provider |
| `loom/src/llm/openai_compat_provider.rs` | loom-specific provider |
| `loom/src/llm/fixed_provider.rs` | loom-specific provider |

### 需要新建

| 文件 | 内容 |
|------|------|
| `loom/src/llm/adapter.rs` | LlmError → AgentError 适配器 |
| `loom/src/llm/traits.rs` | loom 的 LlmClient trait (返回 AgentError) |

### 需要修改

| 文件 | 变更 |
|------|------|
| `loom/src/llm/mod.rs` | 简化为 re-export |
| `loom/src/llm/openai_provider.rs` | 更新导入 |
| `loom/src/llm/openai_compat_provider.rs` | 更新导入 |
| `loom/src/llm/fixed_provider.rs` | 更新导入 |
| `loom/src/state/react_state.rs` | 使用 `loom_llm::ToolCall` |

---

## 实施步骤

### Phase 1: 创建 adapter 和 traits

1. 创建 `loom/src/llm/adapter.rs`
2. 创建 `loom/src/llm/traits.rs`
3. 更新 `loom/src/llm/mod.rs`

### Phase 2: 清理文件

1. 删除 `factory.rs`, `mock.rs`, `model_cache.rs`, `model_registry.rs`
2. 删除 `retry.rs`
3. 删除 `error_classifier/`, `audit.rs`, `thinking.rs`, `tool_call_accumulator.rs`

### Phase 3: 更新现有实现

1. 更新 `openai_provider.rs` 使用新 trait
2. 更新 `openai_compat_provider.rs` 使用新 trait
3. 更新 `fixed_provider.rs` 使用新 trait

### Phase 4: 验证

```bash
cargo check -p loom
cargo test -p loom
```

---

## 关键挑战

### 1. openai/mod.rs 中的 LlmClient

```rust
// 当前: impl LlmClient for ChatOpenAI (返回 AgentError)
// 需要: impl loom_llm::LlmClient for ChatOpenAI (返回 LlmError)
// 然后: impl crate::llm::LlmClient for ChatOpenAI 使用 adapter
```

解决方案：为 ChatOpenAI 添加双重实现。

### 2. 所有使用 `crate::llm::LlmClient` 的地方

需要确保它们使用返回 AgentError 的版本。

### 3. trait 名称冲突

需要使用别名来避免冲突：

```rust
pub use loom_llm::traits::LlmClient as LlmClientTrait;
pub trait LlmClient { ... }  // loom 版本
```

---

## 时间估算

| Phase | 任务 | 时间 |
|-------|------|------|
| 1 | 创建 adapter + traits | 2 小时 |
| 2 | 清理文件 | 1 小时 |
| 3 | 更新实现 | 3 小时 |
| 4 | 验证和修复 | 4 小时 |
| **总计** | | **~10 小时** |

---

## 验证清单

- [ ] `cargo check -p loom-llm`
- [ ] `cargo check -p loom`
- [ ] `cargo test -p loom`
- [ ] 所有 11 个 `impl LlmClient` 编译通过
- [ ] 所有使用 `crate::llm` 的地方编译通过