# 统一 LlmError 和 AgentError 方案

## 目标

将 `AgentError` (loom) 和 `LlmError` (loom-llm) 合并为单一错误类型，消除适配层。

## 方案：移动 AgentError 到 loom-llm

### 最终结构

```
loom-llm/src/
├── lib.rs
├── error.rs              # 统一的错误类型 (原 AgentError)
├── message.rs
├── tool.rs
├── traits.rs             # LlmClient 使用统一错误
└── ...

loom/src/
├── error.rs              # 简化为 re-export: pub use loom_llm::error::AgentError;
├── llm/
│   └── ...
└── ...
```

### 统一错误类型

```rust
// loom-llm/src/error.rs

#[derive(Debug, Error)]
pub enum AgentError {
    // === LLM 错误 ===
    #[error("LLM execution failed: {0}")]
    ExecutionFailed(String),
    
    #[error("LLM returned empty response after {retries} retries")]
    EmptyLlmResponse { retries: u32 },
    
    #[error("run cancelled")]
    Cancelled,
    
    // === Agent 错误 ===
    #[error("tool not found: {0}")]
    ToolNotFound(String),
    
    #[error("invalid tool call: {0}")]
    InvalidToolCall(String),
    
    // ... 其他 AgentError 变体
}

// 实现 LlmError 兼容
impl AgentError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, AgentError::ExecutionFailed(_) | AgentError::RateLimitExceeded)
    }
}
```

---

## 实施步骤

### Phase 1: 迁移 AgentError 到 loom-llm

1. 复制 `loom/src/error.rs` 的 `AgentError` 到 `loom-llm/src/error.rs`
2. 添加 LLM 特定的错误变体
3. 删除 `AgentError::ExecutionFailed` 中的具体 LLM 错误信息

### Phase 2: 更新 loom-llm traits

```rust
// loom-llm/src/traits.rs

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, AgentError>;
    // ...
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn create_client(&self, model: &str) -> Result<Box<dyn LlmClient>, AgentError>;
    // ...
}
```

### Phase 3: 更新 loom/src/error.rs

```rust
//! AgentError - re-export from loom-llm for backward compatibility.

pub use loom_llm::error::AgentError;
```

### Phase 4: 验证和清理

```bash
cargo check -p loom-llm
cargo check -p loom
```

---

## 文件变更清单

### loom-llm/src/error.rs

**操作**: 替换内容，合并 AgentError

```rust
//! Unified error types for Loom and loom-llm.

use thiserror::Error;
use serde::{Deserialize, Serialize};

/// Unified error type for Loom agent operations.
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum AgentError {
    // === LLM errors ===
    #[error("LLM execution failed: {0}")]
    ExecutionFailed(String),
    
    #[error("LLM returned empty response after {retries} retries")]
    EmptyLlmResponse { retries: u32 },
    
    #[error("run cancelled")]
    Cancelled,
    
    // === Tool errors ===
    #[error("tool not found: {0}")]
    ToolNotFound(String),
    
    #[error("tool execution failed: {0}")]
    ToolExecutionFailed(String),
    
    #[error("invalid tool call: {0}")]
    InvalidToolCall(String),
    
    // === State errors ===
    #[error("state error: {0}")]
    StateError(String),
    
    // === Transport errors ===
    #[error("run interrupted: {0}")]
    Interrupted(String),
    
    #[error("rate limit exceeded")]
    RateLimitExceeded,
    
    // === Config errors ===
    #[error("config error: {0}")]
    ConfigError(String),
}

impl AgentError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            AgentError::ExecutionFailed(_) |
            AgentError::RateLimitExceeded
        )
    }
}
```

### loom/src/error.rs

**操作**: 替换为 re-export

```rust
//! Error types for Loom.
//!
//! All errors are re-exported from loom-llm for unified error handling.

pub use loom_llm::error::AgentError;
```

### loom/src/llm/mod.rs

**操作**: 简化，删除重复定义

```rust
//! LLM module.

pub use loom_llm::message::{Message, UserContent, ContentPart, AssistantToolCall, ...};
pub use loom_llm::tool::{ToolCall, ToolSpec};
pub use loom_llm::error::AgentError;
pub use loom_llm::traits::{LlmClient, LlmProvider, LlmResponse, LlmUsage, LlmHeaders, ...};

// Loom-specific implementations
mod openai;
pub use openai::ChatOpenAI;
// ...
```

---

## 验证清单

- [ ] `cargo check -p loom-llm` - 所有 trait 使用统一错误
- [ ] `cargo check -p loom` - backward compatible
- [ ] `grep -r "AgentError" loom/src/` - 所有引用兼容
- [ ] `grep -r "LlmError" loom-llm/src/` - 不再需要 LlmError

---

## 时间估算

| Phase | 任务 | 时间 |
|-------|------|------|
| 1 | 迁移 AgentError 到 loom-llm | 1 小时 |
| 2 | 更新 loom-llm traits | 1 小时 |
| 3 | 更新 loom/src/error.rs | 0.5 小时 |
| 4 | 验证和修复 | 2 小时 |
| **总计** | | **~4.5 小时** |

---

## 优点

1. **单一错误类型** - 无需适配层
2. **统一错误处理** - 所有模块使用相同错误
3. **更清晰的依赖** - loom 依赖 loom-llm，没有循环
4. **更容易维护** - 一个错误类型

## 缺点

1. **破坏性变更** - 所有使用 AgentError 的地方需要确认兼容
2. **loom-llm 依赖 loom 的错误** - 但这正是我们想要的

需要我开始实施吗？