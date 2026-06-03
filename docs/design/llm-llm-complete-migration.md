# loom-llm 完整迁移方案：从空壳到独立 Crate

## 目标

完成 `loom-llm` crate 的完整迁移，使其成为 LLM 层的**唯一权威 crate**。当前 `loom-llm` 仅包含类型骨架（2,859行），所有实现留在 `loom/src/llm`（7,062行）。迁移后 `loom/src/llm` 降为纯 re-export 薄壳（~30行），所有 LLM 客户端、Registry、支撑模块收入 `loom-llm`。

**约束**：`loom-llm` 不依赖 `loom` crate。通过类型统一、trait 下沉、共享工具迁入三条路径消除所有反向依赖。

---

## 1. 现状分析

### 1.1 代码量对比

| 位置 | 行数 | 内容 |
|------|------|------|
| `loom/src/llm/` | **7,062** | 完整实现（客户端、Registry、Audit、ErrorClassifier…） |
| `loom-llm/src/` | **2,859** | 类型定义 + 骨架实现 + 死代码 |
| **合计** | **9,921** | 大量重复，迁移后去重为 ~6,500 行 |

### 1.2 逐文件对比

| 文件 | loom 行数 | loom-llm 行数 | 差异 |
|------|-----------|---------------|------|
| `openai_compat.rs` | **1,573** | 342 | loom-llm 注释 "kept minimal for now" |
| `openai/mod.rs` (ChatOpenAI) | **604** | 362 | 精简版 |
| `model_registry.rs` | **894** | 18 | loom-llm 是空壳 `struct ModelRegistry {}` |
| `retry.rs` | **289** | 176 | loom-llm 是死代码（引用 `crate::types` 不存在路径，未被 mod.rs 声明） |
| `openai/models.rs` | **105** | 14 | 模型列表未搬 |
| `openai/request.rs` | **197** | — | loom-llm 不存在 |
| `openai/stream.rs` | **206** | — | loom-llm 不存在 |

**loom 独有模块**（loom-llm 完全不存在）：

| 模块 | 行数 | 用途 |
|------|------|------|
| `audit.rs` | 608 | LLM 审计日志 |
| `mock.rs` | 274 | MockLlm 测试替身 |
| `thinking.rs` | 224 | 思维链处理 |
| `tool_call_accumulator.rs` | 287 | 工具调用增量累积器 |
| `error_classifier/` | 621 | 错误分类器（OpenAI/BigModel/MiniMax） |
| `factory.rs` | 69 | LLM 工厂 |
| `fixed_provider.rs` | 61 | 固定 Provider |
| `openai_compat_provider.rs` | 61 | Compat Provider |
| `openai_provider.rs` | 61 | OpenAI Provider |
| **小计** | **~2,266** | |

### 1.3 重复类型（不兼容）

| 类型 | loom 定义 | loom-llm 定义 | 兼容性 |
|------|-----------|---------------|--------|
| `ToolSpec` | MCP 格式：`name` + `description` + `input_schema` + `output_hint` | OpenAI function 格式：`tool_type` + `FunctionSpec` | ❌ 完全不同 |
| `ProviderConfig` | 7 字段（`Option<String>` + `fetch_models`） | 6 字段（`String` 类型） | ❌ 字段不同 |
| `ModelEntry` | 13 字段（完整配置 + temperature/max_tokens） | 7 字段（基础标识） | ❌ 子集关系 |

### 1.4 `loom/src/llm` 对 loom crate 的 6 个外部依赖

| # | 依赖 | 使用者 | 行数 | 迁移难度 |
|---|------|--------|------|----------|
| 1 | `crate::tool_source::ToolSpec` | `openai/mod.rs`, `openai_compat.rs`, `openai/request.rs` | — | 🟡 类型统一 |
| 2 | `crate::http_retry::*` | `openai/mod.rs`, `openai_compat.rs` | 120 | 🟢 直接迁入 |
| 3 | `crate::memory::uuid6` | `openai/mod.rs`, `openai_compat.rs`, `audit.rs` | 150 | 🟢 直接迁入 |
| 4 | `crate::model_spec::Provider` | `model_registry.rs` | — | 🟢 加 Cargo 依赖 |
| 5 | `crate::tool_source::ToolSource` trait | `openai/mod.rs` | — | 🟡 trait 下沉 |
| 6 | `crate::state::ToolCall` / `crate::stream::MessageChunk` | `tool_call_accumulator.rs` | — | 🟢 已在 loom-llm |

### 1.5 调用方现状

`loom` crate 内 30+ 处 `use crate::llm::*` 引用，`cli` crate 7 处 `use loom::llm::*`。所有调用方通过 `loom::llm` 路径消费，不直接依赖 `loom_llm`。re-export 层可保持路径不变。

---

## 2. 目标架构

### 2.1 依赖图（迁移后）

```
  model-spec-core (纯数据)
         │
         ▼
  ┌────────────────────────────────────────────────────────┐
  │                    loom-llm (独立)                     │
  │                                                        │
  │  types:   Message, ToolCall, ToolSpec, ToolSource,    │
  │           AgentError, LlmHeaders, LlmResponse,        │
  │           ProviderConfig, ModelEntry, MessageChunk     │
  │                                                        │
  │  client:  ChatOpenAI, ChatOpenAICompat,               │
  │           RetryLlmClient, MockLlm, FixedProvider      │
  │                                                        │
  │  support: http_retry, uuid6, audit,                   │
  │           error_classifier, thinking,                  │
  │           tool_call_accumulator                        │
  │                                                        │
  │  registry: ModelRegistry, create_llm_client,          │
  │            create_llm_provider                         │
  │                                                        │
  │  deps: reqwest, async-openai, model-spec-core, uuid   │
  │  ✗ 不依赖: loom, loom-graph, stream-event             │
  └────────────────────────┬───────────────────────────────┘
                           │
                           ▼
  ┌────────────────────────────────────────────────────────┐
  │                  stream-event                          │
  │  (依赖 loom-llm 的 MessageChunk)                      │
  └────────────────────────┬───────────────────────────────┘
                           │
                           ▼
  ┌────────────────────────────────────────────────────────┐
  │                  loom-graph                            │
  │  (无 loom-llm 依赖，仅用 loom-llm 的 AgentError)      │
  └────────────────────────┬───────────────────────────────┘
                           │
                           ▼
  ┌────────────────────────────────────────────────────────┐
  │                  loom-pregel                           │
  │  (依赖 loom-llm + loom-graph + stream-event)          │
  └────────────────────────┬───────────────────────────────┘
                           │
                           ▼
  ┌────────────────────────────────────────────────────────┐
  │                  loom (re-export 层)                   │
  │  pub use loom_llm::{所有类型和实现};                    │
  │  + LlmFactory (依赖 crate::provider, crate::tier)     │
  └────────────────────────┬───────────────────────────────┘
                           │
                           ▼
  ┌────────────────────────────────────────────────────────┐
  │                  cli                                   │
  │  use loom::llm::*;  (路径不变)                        │
  └────────────────────────────────────────────────────────┘
```

### 2.2 依赖方向规则

- `loom-llm` → `model-spec-core`（纯数据 crate）✅
- `loom-llm` → `reqwest` / `async-openai` / `uuid`（外部 crate）✅
- `loom-llm` → `loom` ❌ **禁止**
- `loom-llm` → `stream-event` ❌ **禁止**（`stream-event` 依赖 `loom-llm`，会产生环）
- `loom` → `loom-llm` ✅

---

## 3. 解耦方案

### 3.1 依赖 1：ToolSpec — 统一到 loom-llm

**问题**：两套 `ToolSpec` 完全不兼容。

**分析**：

- loom 的 `ToolSpec`（MCP 格式）：`name` + `description` + `input_schema` + `output_hint` — 被 `ChatOpenAI`、`ChatOpenAICompat`、`openai/request.rs` 使用
- loom-llm 的 `ToolSpec`（OpenAI function 格式）：`tool_type` + `FunctionSpec` — **从未被任何代码使用**

**方案**：以 loom 的 `ToolSpec` 为准，迁入 `loom-llm/src/tool.rs`，替换现有空壳。`ToolOutputHint` 枚举一并迁入（仅 3 个变体，无外部依赖）。

```rust
// loom-llm/src/tool.rs — 迁入后的 ToolSpec

/// Tool specification for LLM function calling.
/// Follows MCP inputSchema format.
pub struct ToolSpec {
    /// Tool name (e.g. "bash", "read_file").
    pub name: String,
    /// Human-readable description for the LLM.
    pub description: Option<String>,
    /// JSON Schema for arguments (MCP inputSchema).
    pub input_schema: serde_json::Value,
    /// Optional output normalization hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_hint: Option<ToolOutputHint>,
}
```

loom 的 `ToolSpec` → OpenAI `ChatCompletionTools` 转换逻辑在 `openai/request.rs` 中，一并迁入。

**影响**：`loom/src/tool_source/mod.rs` 中 `ToolSpec` 定义改为 `pub use loom_llm::tool::ToolSpec`。

### 3.2 依赖 2：http_retry — 整体迁入

**问题**：`loom/src/http_retry.rs`（120行）被 `openai/mod.rs` 和 `openai_compat.rs` 引用。

**分析**：纯工具函数，仅依赖 `reqwest`（`loom-llm` 已有）和 `Duration`。无任何 loom crate 依赖。

**方案**：整体迁入 `loom-llm/src/support/http_retry.rs`。`loom/src/http_retry.rs` 改为 re-export。

```
loom-llm/src/support/http_retry.rs   ← 原始实现（120行）
loom/src/http_retry.rs                ← pub use loom_llm::support::http_retry::*;
```

### 3.3 依赖 3：uuid6 — 整体迁入

**问题**：`loom/src/memory/uuid6.rs`（150行）被 `openai/mod.rs`、`openai_compat.rs`、`audit.rs` 引用。

**分析**：纯 UUID 生成，依赖 `uuid` crate（`loom-llm` 已有）+ `sha2` + `mac_address`。无 loom crate 依赖。

**方案**：迁入 `loom-llm/src/support/uuid6.rs`。`loom/src/memory/uuid6.rs` 改为 re-export。

`loom-llm/Cargo.toml` 新增：
```toml
sha2 = "0.10"
mac_address = "1.1"
```

### 3.4 依赖 4：model_spec::Provider — Cargo 依赖

**问题**：`model_registry.rs` 用 `model_spec::Provider`（来自 `model-spec-core` crate）解析 models.dev 模型目录。

**方案**：`loom-llm/Cargo.toml` 直接添加 `model-spec-core` 依赖。

```toml
# loom-llm/Cargo.toml
model-spec-core = { path = "../model-spec-core" }
```

`model-spec-core` 是纯数据 crate（无 `loom` 依赖），不会引入反向依赖。

`model_registry.rs` 迁入后 `use crate::model_spec` → `use model_spec_core::spec::Provider as SpecProvider`。

### 3.5 依赖 5：ToolSource trait — trait 下沉

**问题**：`ChatOpenAI::new_with_tool_source()` 接收 `&dyn ToolSource`。`ToolSource` trait 定义在 `loom/src/tool_source/mod.rs`，有 20+ 个具体实现（McpToolSource、BashToolsSource 等）。

**分析**：`ToolSource` trait 本身仅依赖 `ToolSpec` + `AgentError`，迁移后都在 loom-llm 内部。具体实现（McpToolSource 等）依赖大量 loom 内部模块，留在 loom。

**方案**：trait 定义迁入 `loom-llm/src/tool.rs`，具体实现留在 loom。

```rust
// loom-llm/src/tool.rs — trait 下沉

/// Tool source trait: resolves tool specifications for LLM clients.
/// Implementations stay in loom crate; only the trait lives here.
#[async_trait]
pub trait ToolSource: Send + Sync {
    /// List all tools available for the LLM.
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError>;
    // ...
}
```

```rust
// loom/src/tool_source/mod.rs — re-export trait + 保留实现
pub use loom_llm::tool::{ToolSource, ToolSourceError};

// 具体实现仍在 loom
mod mcp;
mod bash_tools_source;
// ...
```

### 3.6 依赖 6：ToolCall / MessageChunk — 已在 loom-llm

**问题**：`tool_call_accumulator.rs` 引用 `crate::state::ToolCall`。

**分析**：
- `ToolCall` → 定义在 `loom-llm/src/tool.rs`（loom 通过 `type ToolCall = loom_llm::ToolCall` 别名引用）
- `MessageChunk` → 定义在 `loom-llm/src/traits.rs`

**方案**：`tool_call_accumulator.rs` 迁入 loom-llm 后改为 `use crate::tool::ToolCall`。无需搬运。

### 3.7 解耦方案汇总

| 依赖 | 方案 | 难度 | 改动量 |
|------|------|------|--------|
| `ToolSpec` | 统一到 loom-llm（以 loom 版为准） | 🟡 | 替换 `loom-llm/src/tool.rs`，更新 `loom/src/tool_source/mod.rs` |
| `http_retry` | 整体迁入 loom-llm | 🟢 | 移文件 + re-export |
| `uuid6` | 整体迁入 loom-llm | 🟢 | 移文件 + re-export + Cargo.toml 加 sha2/mac_address |
| `model_spec::Provider` | loom-llm 直接依赖 model-spec-core | 🟢 | Cargo.toml 加一行 |
| `ToolSource` trait | trait 下沉到 loom-llm | 🟡 | 移 trait 定义 + re-export |
| `ToolCall` / `MessageChunk` | 已在 loom-llm，改 import 路径 | 🟢 | 路径替换 |

---

## 4. loom-llm 目录结构（迁移后）

```
loom-llm/
├── Cargo.toml
└── src/
    ├── lib.rs                              # crate root + 全量 re-export
    
    │── error.rs                            # AgentError, Interrupt（已有）
    ├── message.rs                          # Message, UserContent, ...（已有）
    ├── tool.rs                             # ToolCall, ToolSpec, ToolSource trait（扩展）
    ├── traits.rs                           # LlmClient, LlmProvider, LlmResponse...（精简，删除重复类型）
    
    ├── client/
    │   ├── mod.rs
    │   ├── openai/
    │   │   ├── mod.rs                      # ChatOpenAI（完整版，从 loom 迁入）
    │   │   ├── models.rs                   # 模型列表（从 loom 迁入）
    │   │   ├── request.rs                  # 请求构建（从 loom 迁入）
    │   │   ├── stream.rs                   # 流式解析（从 loom 迁入）
    │   │   └── tests.rs                    # 测试（从 loom 迁入）
    │   ├── openai_compat.rs                # ChatOpenAICompat（完整版，替换骨架）
    │   ├── retry.rs                        # RetryLlmClient（完整版，替换死代码）
    │   ├── mock.rs                         # MockLlm（从 loom 迁入）
    │   ├── fixed_provider.rs               # FixedLlmProvider（从 loom 迁入）
    │   ├── openai_provider.rs              # OpenAIProvider（从 loom 迁入）
    │   ├── openai_compat_provider.rs       # OpenAICompatProvider（从 loom 迁入）
    │   ├── tool_call_accumulator.rs        # （从 loom 迁入）
    │   └── thinking.rs                     # （从 loom 迁入）
    
    ├── support/
    │   ├── mod.rs
    │   ├── http_retry.rs                   # （从 loom/src/http_retry.rs 迁入）
    │   ├── uuid6.rs                        # （从 loom/src/memory/uuid6.rs 迁入）
    │   ├── audit.rs                        # （从 loom/src/llm/audit.rs 迁入）
    │   └── error_classifier/
    │       ├── mod.rs                      # （从 loom/src/llm/error_classifier/ 迁入）
    │       ├── openai.rs
    │       ├── bigmodel.rs
    │       └── minimax.rs
    
    └── registry.rs                         # ModelRegistry, ProviderConfig, ModelEntry,
                                            # create_llm_client, create_llm_provider
                                            # （完整版，替换空壳）
```

### Cargo.toml（迁移后）

```toml
[package]
name = "loom-llm"
version.workspace = true
edition.workspace = true
description = "LLM client abstractions for Loom agents"
license.workspace = true
authors.workspace = true

[lib]
path = "src/lib.rs"

[features]
default = []
testing = []

[dependencies]
# 异步运行时
tokio = { workspace = true }
async-trait = { workspace = true }

# 序列化
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# 日志
tracing = "0.1"

# HTTP 客户端
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }

# OpenAI 客户端
async-openai = { version = "0.32", features = ["chat-completion", "model"] }

# 模型规格
model-spec-core = { path = "../model-spec-core" }           # ← 新增

# UUID + uuid6 支持
uuid = { version = "1", features = ["v4"] }
sha2 = "0.10"                                                # ← 新增（uuid6 依赖）
mac_address = "1.1"                                          # ← 新增（uuid6 依赖）

# 并发
dashmap = "6.0"

[dev-dependencies]
tokio-test = "0.4"
tempfile = "3"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

**删除的依赖**（不再需要）：
- `thiserror` — 已改用 `serde` 序列化错误
- `tokio-stream`, `futures-util`, `futures`, `once_cell`, `dirs` — 未使用
- `lancedb` — 未使用的 optional 依赖

---

## 5. loom crate 改动

### 5.1 `loom/src/llm/mod.rs` — 纯 re-export 薄壳

```rust
//! LLM module — re-exports from loom-llm crate.
//! All implementations live in loom-llm; this module preserves backward compatibility.

// Re-export all public types from loom-llm
pub use loom_llm::{
    // Core types
    AgentError, Interrupt, GraphInterrupt,
    Message, UserContent, ContentPart, ContentError,
    AssistantPayload, AssistantToolCall, ToolCallContent,
    assistant_content_for_chat_api,
    ToolCall, ToolSpec, ToolSource, ToolSourceError,
    ToolOutputHint,
    LlmClient, LlmProvider, LlmResponse, LlmUsage, LlmHeaders,
    ToolChoiceMode, ToolCallDelta, ModelInfo, ModelCapabilities,
    PromptTokensDetails, CompletionTokensDetails,
    MessageChunk, MessageChunkKind,
    ProviderConfig, ModelEntry,

    // Clients
    ChatOpenAI, ChatOpenAICompat, RetryLlmClient,
    OpenAIProvider, OpenAICompatProvider, FixedLlmProvider,
    CloneableLlmClient,
    MockLlm, MultiRoundMockLlm,

    // Registry
    ModelRegistry, CachedModelList, CombinedModelList,
    create_llm_client, create_llm_provider,

    // Support
    support::{audit::LlmAuditLog, error_classifier, thinking},
};

/// LLM factory that resolves tiers and creates clients.
/// Kept in loom because it depends on crate::provider and crate::tier.
mod factory;
pub use factory::LlmFactory;

#[deprecated(note = "renamed to ChatOpenAICompat")]
pub type ChatBigModel = ChatOpenAICompat;

pub fn get_headers_from_env() -> LlmHeaders {
    LlmHeaders::from_env()
}
```

### 5.2 `loom/src/lib.rs` — 更新 re-export 列表

```rust
// 改前:
pub use llm::{
    ChatOpenAI, ChatOpenAICompat, CompletionTokensDetails, FixedLlmProvider, LlmClient,
    LlmProvider, LlmResponse, LlmUsage, MockLlm, MultiRoundMockLlm, OpenAICompatProvider,
    OpenAIProvider, PromptTokensDetails, ToolCallDelta, ToolChoiceMode,
};

// 改后: (保持不变，从 loom::llm re-export 传递)
pub use llm::{
    ChatOpenAI, ChatOpenAICompat, CompletionTokensDetails, FixedLlmProvider, LlmClient,
    LlmProvider, LlmResponse, LlmUsage, MockLlm, MultiRoundMockLlm, OpenAICompatProvider,
    OpenAIProvider, PromptTokensDetails, ToolCallDelta, ToolChoiceMode,
};
```

### 5.3 `loom/src/tool_source/mod.rs` — 删除本地 ToolSpec 定义

```rust
// 改前:
pub struct ToolSpec { name, description, input_schema, output_hint }

// 改后:
pub use loom_llm::tool::{ToolSpec, ToolOutputHint, ToolSource, ToolSourceError};
```

### 5.4 `loom/src/http_retry.rs` — 改为 re-export

```rust
//! HTTP retry utilities — re-exported from loom-llm.
pub use loom_llm::support::http_retry::*;
```

### 5.5 `loom/src/memory/uuid6.rs` — 改为 re-export

```rust
//! UUID6 generation — re-exported from loom-llm.
pub use loom_llm::support::uuid6::*;
```

### 5.6 删除的文件/目录

迁移完成后删除 `loom/src/llm/` 下所有 `.rs` 文件（只留 `mod.rs` + `factory.rs`）：

```
删除:
  loom/src/llm/audit.rs
  loom/src/llm/error_classifier/       (整个目录)
  loom/src/llm/fixed_provider.rs
  loom/src/llm/mock.rs
  loom/src/llm/model_registry.rs
  loom/src/llm/openai/                 (整个目录)
  loom/src/llm/openai_compat.rs
  loom/src/llm/openai_compat_provider.rs
  loom/src/llm/openai_provider.rs
  loom/src/llm/retry.rs
  loom/src/llm/thinking.rs
  loom/src/llm/tool_call_accumulator.rs

保留:
  loom/src/llm/mod.rs                  (re-export 薄壳)
  loom/src/llm/factory.rs              (依赖 crate::provider, crate::tier)
```

---

## 6. 数据流对比

### 6.1 迁移前

```
调用方 (loom/src/agent/react/build/llm.rs)
  │
  use crate::llm::{ChatOpenAI, create_llm_client, ...}
  │
  ▼
loom/src/llm/mod.rs
  ├── pub use loom_llm::{Message, ToolCall, LlmClient, ...}     ← 类型 re-export
  ├── pub mod openai_compat;                                     ← 实现在本地 (1,573行)
  ├── pub mod openai;                                            ← 实现在本地 (604行)
  ├── pub mod model_registry;                                    ← 实现在本地 (894行)
  ├── pub mod retry;                                             ← 实现在本地 (289行)
  ├── pub mod audit;                                             ← 实现在本地 (608行)
  ├── pub mod error_classifier;                                  ← 实现在本地 (621行)
  └── pub mod mock, thinking, tool_call_accumulator, ...         ← 实现在本地

loom-llm/src/
  ├── message.rs, tool.rs, traits.rs, error.rs                   ← 类型定义（被 re-export）
  ├── client/openai_compat.rs                                    ← 精简骨架（342行，无人使用）
  ├── client/openai/                                             ← 精简骨架（无人使用）
  ├── client/retry.rs                                            ← 死代码（引用 crate::types 不存在）
  └── registry.rs                                                ← 空壳（18行）
```

### 6.2 迁移后

```
调用方 (loom/src/agent/react/build/llm.rs)
  │
  use crate::llm::{ChatOpenAI, create_llm_client, ...}          ← 路径不变
  │
  ▼
loom/src/llm/mod.rs
  ├── pub use loom_llm::{所有类型和实现}                          ← 纯 re-export (~30行)
  └── mod factory;                                               ← 唯一本地模块 (69行)

loom-llm/src/
  ├── message.rs, tool.rs, traits.rs, error.rs                   ← 类型定义（权威来源）
  ├── client/openai_compat.rs                                    ← 完整实现 (1,573行)
  ├── client/openai/                                             ← 完整实现 (604+行)
  ├── client/retry.rs                                            ← 完整实现 (289行)
  ├── client/mock.rs                                             ← 完整实现 (274行)
  ├── support/audit.rs                                           ← 完整实现 (608行)
  ├── support/error_classifier/                                  ← 完整实现 (621行)
  ├── support/http_retry.rs                                      ← 从 loom 迁入 (120行)
  ├── support/uuid6.rs                                           ← 从 loom 迁入 (150行)
  └── registry.rs                                                ← 完整实现 (894行)
```

---

## 7. Edge Cases

### 7.1 `LlmFactory` 的归属

`factory.rs` 引用 `crate::provider::load_provider_configs()` 和 `crate::tier::plan::tier_plans()`，这两个模块是 loom 胶水层。**方案**：`LlmFactory` 留在 `loom/src/llm/factory.rs`，不迁入 loom-llm。它是唯一需要留在 loom 的模块（69行）。

### 7.2 `stream-event` 对 `MessageChunk` 的依赖

`stream-event` crate 通过 `pub use loom_llm::traits::{MessageChunk, MessageChunkKind}` 引用。迁移后类型定义仍在 `loom-llm`，`stream-event` 无需改动。但需确保 `loom-llm` 不反向依赖 `stream-event`（当前无此依赖，保持即可）。

### 7.3 `loom-graph` 对 `loom-llm` 的依赖

`loom-graph` 仅依赖 `loom-llm` 的 `AgentError` 和 `Interrupt`。迁移后这些类型仍在 `loom-llm`，`loom-graph` 无需改动。

### 7.4 `ToolSpec` 双版本清理

`loom-llm` 现有的 OpenAI function 格式 `ToolSpec`（含 `FunctionSpec`）需**删除**。验证无人使用：

- `loom-llm/src/client/openai_compat.rs` 中的精简版不用它
- `loom` 代码不用它
- 无任何 crate 通过 `loom_llm::tool::ToolSpec` 引用它

### 7.5 `ProviderConfig` / `ModelEntry` 统一

以 `loom/src/llm/model_registry.rs` 中的定义为准（功能更全）。`loom-llm/src/traits.rs` 中的精简版定义**删除**，改为从 `registry.rs` re-export。所有字段必须完整迁移：

```
ProviderConfig: name, base_url(Option), api_key(Option), provider_type(Option),
                fetch_models(bool), cache_ttl(Option), enable_tier_resolution(bool)

ModelEntry: id, name, provider, base_url(Option), api_key(Option), provider_type(Option),
            temperature(Option), max_tokens(Option), tool_choice(Option),
            family(Option), version(Option)
```

### 7.6 `ToolSourceError` 下沉

`ToolSource` trait 引用 `ToolSourceError`。当前定义在 `loom/src/tool_source/mod.rs`。需一并迁入 `loom-llm/src/tool.rs`。`ToolSourceError` 仅依赖 `String`，无外部依赖。

---

## 8. 性能影响

| 维度 | 影响 |
|------|------|
| **编译时间** | `loom-llm` 增加 `model-spec-core`、`sha2`、`mac_address` 依赖。均为纯数据/crypto crate，编译增量 < 5s |
| **运行时** | 零影响。代码只是换了位置，逻辑不变 |
| **二进制大小** | `loom-llm` crate 编译产物略增（因增加了 uuid6 的 sha2/mac_address 依赖），但 loom 总 binary 不变（原本就间接依赖） |
| **编译并行度** | 提升。修改 `loom/src/agent/` 不再触发 `loom-llm` 重编译（当前是同一 crate 内的 re-export，改动任何文件都会重编译整个 loom） |

---

## 9. 实施步骤

### Phase 1：类型统一 + 清理死代码

**目标**：消除重复类型定义，删除死代码。确保 loom-llm 编译通过。

| 步骤 | 操作 | 验证 |
|------|------|------|
| 1.1 | `loom-llm/src/traits.rs` 中删除重复的 `ProviderConfig` 和 `ModelEntry` 定义 | — |
| 1.2 | 将 loom 的 `ProviderConfig`（7 字段）和 `ModelEntry`（13 字段）迁入 `loom-llm/src/registry.rs`（替换空壳） | — |
| 1.3 | 将 loom 的 `ToolSpec`（MCP 格式）+ `ToolOutputHint` 迁入 `loom-llm/src/tool.rs`（替换 OpenAI 格式空壳） | — |
| 1.4 | 将 `ToolSource` trait + `ToolSourceError` 迁入 `loom-llm/src/tool.rs` | — |
| 1.5 | 删除 `loom-llm/src/client/retry.rs`（死代码，引用不存在的 `crate::types`） | — |
| 1.6 | 删除 `loom-llm/src/client/openai/`（精简骨架，将被完整版替换） | — |
| 1.7 | 更新 `loom-llm/src/lib.rs` re-export | `cargo check -p loom-llm` ✅ |
| 1.8 | 更新 `loom/src/llm/mod.rs`、`loom/src/tool_source/mod.rs` 的 re-export | `cargo check -p loom` ✅ |

**风险**：🟢 低 — 仅类型搬移，不改逻辑。

### Phase 2：支撑模块迁入

**目标**：将零耦合的工具模块迁入 loom-llm。

| 步骤 | 操作 | 验证 |
|------|------|------|
| 2.1 | `loom/src/http_retry.rs` → `loom-llm/src/support/http_retry.rs` | — |
| 2.2 | `loom/src/memory/uuid6.rs` → `loom-llm/src/support/uuid6.rs` | — |
| 2.3 | `loom-llm/Cargo.toml` 增加 `sha2`、`mac_address` | — |
| 2.4 | `loom/src/llm/error_classifier/` → `loom-llm/src/support/error_classifier/` | — |
| 2.5 | `loom/src/llm/thinking.rs` → `loom-llm/src/client/thinking.rs` | — |
| 2.6 | `loom/src/llm/tool_call_accumulator.rs` → `loom-llm/src/client/tool_call_accumulator.rs`（改 `crate::state::ToolCall` → `crate::tool::ToolCall`） | — |
| 2.7 | `loom/src/llm/audit.rs` → `loom-llm/src/support/audit.rs`（改 `crate::memory::uuid6` → `crate::support::uuid6`） | — |
| 2.8 | loom 原位置改为 re-export | `cargo check -p loom-llm && cargo check -p loom` ✅ |

**风险**：🟢 低 — 6 个模块中有 5 个零外部依赖（仅 `uuid6` 需加 Cargo 依赖）。

### Phase 3：核心客户端迁入

**目标**：将 LLM 客户端实现迁入 loom-llm。

| 步骤 | 操作 | 改动 |
|------|------|------|
| 3.1 | `loom/src/llm/openai/`（完整版）→ `loom-llm/src/client/openai/` | `crate::error` → `crate::error`，`crate::http_retry` → `crate::support::http_retry`，`crate::memory::uuid6` → `crate::support::uuid6`，`crate::message` → `crate::message`，`crate::tool_source::ToolSpec` → `crate::tool::ToolSpec`，`crate::stream::MessageChunk` → `crate::traits::MessageChunk` |
| 3.2 | `loom/src/llm/openai_compat.rs`（完整版）→ `loom-llm/src/client/openai_compat.rs` | 同上路径替换 |
| 3.3 | `loom/src/llm/retry.rs`（完整版）→ `loom-llm/src/client/retry.rs` | `crate::error` → `crate::error` |
| 3.4 | `loom/src/llm/mock.rs` → `loom-llm/src/client/mock.rs` | `crate::state::ToolCall` → `crate::tool::ToolCall`，`crate::stream::MessageChunk` → `crate::traits::MessageChunk` |
| 3.5 | `loom/src/llm/fixed_provider.rs` → `loom-llm/src/client/fixed_provider.rs` | `crate::error` → `crate::error` |
| 3.6 | `loom/src/llm/openai_provider.rs` → `loom-llm/src/client/openai_provider.rs` | `crate::llm::model_registry::*` → `crate::registry::*` |
| 3.7 | `loom/src/llm/openai_compat_provider.rs` → `loom-llm/src/client/openai_compat_provider.rs` | 同上 |
| 3.8 | 更新 `loom-llm/src/client/mod.rs` 声明所有新模块 | — |

**验证**：`cargo check -p loom-llm && cargo check -p loom` ✅

**风险**：🟡 中 — `openai/mod.rs`（604行）和 `openai_compat.rs`（1,573行）路径替换较多，但都是 `use` 语句替换。

### Phase 4：ModelRegistry 迁入

**目标**：将完整的 ModelRegistry 迁入 loom-llm。

| 步骤 | 操作 | 改动 |
|------|------|------|
| 4.1 | `loom-llm/Cargo.toml` 增加 `model-spec-core` 依赖 | — |
| 4.2 | `loom/src/llm/model_registry.rs` → `loom-llm/src/registry.rs`（替换空壳） | `crate::model_spec::Provider` → `model_spec_core::spec::Provider`，`crate::llm::*` → `crate::*` |
| 4.3 | 更新 `loom-llm/src/lib.rs` re-export registry 类型 | — |

**验证**：`cargo check -p loom-llm && cargo check -p loom` ✅

**风险**：🟡 中 — `model_registry.rs`（894行）是最大的单文件，但依赖替换路径明确。

### Phase 5：loom/src/llm 转薄壳 + 全量验证

**目标**：`loom/src/llm` 降为 re-export 薄壳，全量编译测试通过。

| 步骤 | 操作 |
|------|------|
| 5.1 | 重写 `loom/src/llm/mod.rs` 为纯 re-export（见 §5.1） |
| 5.2 | 删除 `loom/src/llm/` 下所有已迁移的文件（见 §5.6 列表） |
| 5.3 | 保留 `loom/src/llm/factory.rs`（唯一本地模块） |
| 5.4 | 更新 `loom/src/tool_source/mod.rs`（ToolSpec → re-export） |
| 5.5 | 更新 `loom/src/http_retry.rs`（re-export） |
| 5.6 | 更新 `loom/src/memory/uuid6.rs`（re-export） |
| 5.7 | `cargo check --workspace` 全量编译 |
| 5.8 | `cargo test --workspace` 全量测试 |

**风险**：🟡 中 — 删除文件可能遗漏更新某些 `use crate::llm::*` 引用。通过编译错误逐一修复。

---

## 10. 收益

| 维度 | 迁移前 | 迁移后 |
|------|--------|--------|
| **`loom/src/llm` 行数** | 7,062 | ~100（re-export + factory） |
| **`loom-llm/src` 行数** | 2,859（骨架 + 死代码） | ~6,500（完整实现） |
| **重复类型** | 3 套（ToolSpec ×2, ProviderConfig ×2, ModelEntry ×2） | 0 |
| **死代码** | `client/retry.rs`（引用不存在路径） | 0 |
| **`cargo check -p loom-llm`** | 编译通过但无实际功能 | 完整可用的独立 LLM crate |
| **独立可复用** | ❌ 只能通过 loom 使用 | ✅ 第三方可直接 `loom-llm = "..."` |
| **编译隔离** | 改 agent 触发 llm 重编译 | 改 agent 不触发 `loom-llm` 重编译 |
| **依赖清晰度** | loom-llm 的真正依赖不明 | 明确：reqwest + async-openai + model-spec-core |
