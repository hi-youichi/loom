# loom-llm 完整迁移方案：删除 `loom/src/llm`

## 目标

将 `loom/src/llm/` 下所有实现代码迁入 `loom-llm` crate，使 `loom/src/llm/` 降为纯 re-export 薄壳（~15行），`loom-llm` 成为 LLM 层的**唯一权威 crate**。

**约束**：`loom-llm` 不依赖 `loom` crate。通过类型统一、依赖注入消除所有反向依赖。

---

## 1. 现状分析

### 1.1 已完成迁移（Phase 1-3）

以下模块已成功迁入 `loom-llm`，loom 侧改为 re-export：

| 模块 | 行数 | loom-llm 位置 | 状态 |
|------|------|---------------|------|
| `ToolSpec` + `ToolOutputHint` + `ToolOutputStrategy` + `ToolSourceError` | ~231 | `loom-llm/src/tool.rs` | ✅ 已迁入 |
| `ProviderConfig` + `ModelEntry` + `CachedModelList` + `CombinedModelList` | ~320 | `loom-llm/src/registry.rs` | ✅ 已迁入 |
| `http_retry` | 120 | `loom-llm/src/support/http_retry.rs` | ✅ 已迁入 |
| `uuid6` | 361 | `loom-llm/src/support/uuid6.rs` | ✅ 已迁入 |
| `thinking` | 224 | `loom-llm/src/support/thinking.rs` | ✅ 已迁入 |
| `tool_call_accumulator` | 287 | `loom-llm/src/support/tool_call_accumulator.rs` | ✅ 已迁入 |
| `error_classifier/` | ~239 | `loom-llm/src/support/error_classifier/` | ✅ 已迁入 |
| `RetryLlmClient` | 289 | `loom-llm/src/client/retry.rs` | ✅ 已迁入 |
| `MockLlm` + `MultiRoundMockLlm` | 274 | `loom-llm/src/client/mock.rs` | ✅ 已迁入 |
| `FixedLlmProvider` | 61 | `loom-llm/src/client/fixed_provider.rs` | ✅ 已迁入 |
| `ChatOpenAICompat`（简化版） | 342 | `loom-llm/src/client/openai_compat.rs` | ⚠️ 骨架版 |

### 1.2 待迁入文件（本次目标）

`loom/src/llm/` 当前剩余 **4,987 行**：

| 文件 | 行数 | 用途 | 关键 loom 依赖 |
|------|------|------|---------------|
| `openai_compat.rs` | 1,573 | 完整 OpenAI 兼容客户端（SSE 流式、重试、审计、思考解析） | `crate::tool_source::ToolSource`, `crate::state::ToolCall`, `crate::message` |
| `openai/mod.rs` | 604 | ChatOpenAI 客户端（via async-openai） | `crate::tool_source::ToolSource`, `crate::state::ToolCall`, `crate::llm::audit` |
| `model_registry.rs` | 574 | ModelRegistry 运行时 + `create_llm_client` / `create_llm_provider` | `crate::model_spec::Provider`, `ChatOpenAI`, `ChatOpenAICompat` |
| `audit.rs` | 608 | LLM 审计日志系统 | `crate::memory::uuid6` → 已在 loom-llm |
| `openai/stream.rs` | 432 | SSE 流式解析 | `crate::llm::thinking`, `crate::llm::tool_call_accumulator` |
| `openai/tests.rs` | 442 | ChatOpenAI 测试 | `crate::test_util` |
| `openai/request.rs` | 377 | 请求体构建 | `crate::tool_source::ToolSpec`, `crate::message` |
| `openai/models.rs` | 105 | OpenAI 模型列表 | 无 loom 特有依赖 |
| `factory.rs` | 69 | LlmFactory（tier 解析 + client 创建） | `crate::provider`, `crate::tier` |
| `openai_provider.rs` | 61 | OpenAI LlmProvider 实现 | `ChatOpenAI`, `ModelEntry` |
| `openai_compat_provider.rs` | 61 | OpenAI Compat LlmProvider 实现 | `ChatOpenAICompat`, `ModelEntry` |
| `mod.rs` | 81 | 模块入口 + re-export | — |

### 1.3 代码量对比

| 位置 | 当前行数 | 迁移后行数 |
|------|---------|-----------|
| `loom/src/llm/` | **4,987** | ~15（纯 re-export 薄壳） |
| `loom-llm/src/` | **4,526** | ~9,500（完整实现） |

### 1.4 关键依赖：`ToolSource` trait

`ToolSource` trait 定义在 `loom/src/tool_source/mod.rs`，依赖 `ToolCallContext`：

```rust
// ToolCallContext 的 loom 依赖（无法下沉到 loom-llm）
use crate::cli_run::AnyStreamEvent;    // loom CLI 事件类型
use crate::message::Message;           // loom 消息类型（已迁 loom-llm）
use crate::stream::ToolStreamWriter;   // loom 流写入器
use crate::RunCancellation;            // loom 运行时取消
```

**结论**：`ToolSource` trait + `ToolCallContext` 必须留在 `loom`。但 `ChatOpenAI` 和 `ChatOpenAICompat` 对 `ToolSource` 的使用**仅限于构造时**的 `new_with_tool_source()` 方法——调用 `tool_source.list_tools()` 获取 `Vec<ToolSpec>`，之后不再持有 `ToolSource` 引用。

**解法**：客户端迁入 loom-llm 后，`new_with_tool_source()` 保留为 loom 侧的扩展方法（见 §3.2）。

---

## 2. 目标架构

### 2.1 依赖图（迁移后）

```
  model-spec-core (纯数据 crate)
         │
         ▼
  ┌──────────────────────────────────────────────────────────┐
  │                     loom-llm (独立)                      │
  │                                                          │
  │  types:    Message, ToolCall, ToolSpec, AgentError,      │
  │            LlmHeaders, LlmResponse, ProviderConfig,      │
  │            ModelEntry, MessageChunk                       │
  │                                                          │
  │  client:   ChatOpenAI, ChatOpenAICompat,                 │
  │            OpenAIProvider, OpenAICompatProvider,          │
  │            RetryLlmClient, MockLlm, FixedLlmProvider     │
  │                                                          │
  │  support:  http_retry, uuid6, audit, thinking,           │
  │            tool_call_accumulator, error_classifier        │
  │                                                          │
  │  registry: ModelRegistry, create_llm_client,             │
  │            create_llm_provider                           │
  │                                                          │
  │  deps:     reqwest, async-openai, model-spec-core, uuid  │
  │  ✗ 不依赖: loom, loom-graph, stream-event               │
  └──────────────────────┬───────────────────────────────────┘
                         │
                         ▼
  ┌──────────────────────────────────────────────────────────┐
  │                    stream-event                          │
  │  (依赖 loom-llm 的 MessageChunk)                        │
  └──────────────────────┬───────────────────────────────────┘
                         │
                         ▼
  ┌──────────────────────────────────────────────────────────┐
  │                    loom-graph                            │
  │  (无 loom-llm 依赖)                                     │
  └──────────────────────┬───────────────────────────────────┘
                         │
                         ▼
  ┌──────────────────────────────────────────────────────────┐
  │                    loom-pregel                           │
  │  (依赖 loom-llm + loom-graph + stream-event)            │
  └──────────────────────┬───────────────────────────────────┘
                         │
                         ▼
  ┌──────────────────────────────────────────────────────────┐
  │                    loom (re-export + 胶水)               │
  │  pub use loom_llm::{所有类型和实现};                      │
  │  + LlmFactory (依赖 crate::provider, crate::tier)       │
  │  + ToolSource trait (依赖 ToolCallContext)               │
  └──────────────────────┬───────────────────────────────────┘
                         │
                         ▼
  ┌──────────────────────────────────────────────────────────┐
  │                    cli / loom-acp / serve                │
  │  use loom::llm::*;  (路径不变)                          │
  └──────────────────────────────────────────────────────────┘
```

### 2.2 依赖方向规则

| 规则 | 说明 |
|------|------|
| `loom-llm` → `model-spec-core` | ✅ 纯数据 crate，无反向依赖 |
| `loom-llm` → `reqwest` / `async-openai` / `uuid` | ✅ 外部 crate |
| `loom-llm` → `loom` | ❌ **禁止** |
| `loom-llm` → `stream-event` | ❌ **禁止**（会产生循环） |
| `loom` → `loom-llm` | ✅ |

---

## 3. 解耦方案

### 3.1 依赖清单与解法

`loom/src/llm/` 剩余文件对 loom crate 的所有依赖：

| # | 依赖 | 使用者 | 解法 | 难度 |
|---|------|--------|------|------|
| 1 | `crate::tool_source::ToolSource` trait | `openai/mod.rs` L123, `openai_compat.rs` L352 | 构造时注入 `Vec<ToolSpec>`，不依赖 trait（见 §3.2） | 🟡 |
| 2 | `crate::tool_source::ToolSourceError` | 同上 | 已在 `loom-llm/src/tool.rs`，改 import 路径 | 🟢 |
| 3 | `crate::tool_source::ToolSpec` | `openai/request.rs` L23 | 已在 `loom-llm/src/tool.rs`，改 import 路径 | 🟢 |
| 4 | `crate::model_spec::Provider` | `model_registry.rs` L25 | `loom-llm` 加 `model-spec-core` Cargo 依赖 | 🟢 |
| 5 | `crate::memory::uuid6` | `audit.rs` L14 | 已在 `loom-llm/src/support/uuid6.rs`，改 import 路径 | 🟢 |
| 6 | `crate::state::ToolCall` | `openai/mod.rs` L33, `openai_compat.rs` L36 | 已在 `loom-llm/src/tool.rs`，改 import 路径 | 🟢 |
| 7 | `crate::stream::MessageChunk` | `openai_compat.rs` L37, `openai/stream.rs` L17 | 已在 `loom-llm/src/traits.rs`，改 import 路径 | 🟢 |
| 8 | `crate::message::Message` | `openai/mod.rs` L32, `openai/request.rs` L22 | 已在 `loom-llm/src/message.rs`，改 import 路径 | 🟢 |
| 9 | `crate::error::AgentError` | 多处 | 已在 `loom-llm/src/error.rs`，改 import 路径 | 🟢 |
| 10 | `crate::http_retry::*` | `openai/mod.rs` L23, `openai_compat.rs` L29 | 已在 `loom-llm/src/support/http_retry.rs`，改 import 路径 | 🟢 |
| 11 | `crate::llm::audit::*` | `openai/mod.rs` L26-29 | 将 `audit.rs` 迁入 `loom-llm`（见 §3.3） | 🟢 |
| 12 | `crate::llm::thinking::*` | `openai/stream.rs` L12 | 已在 `loom-llm/src/support/thinking.rs` | 🟢 |
| 13 | `crate::llm::tool_call_accumulator::*` | `openai/stream.rs` L15 | 已在 `loom-llm/src/support/tool_call_accumulator.rs` | 🟢 |
| 14 | `crate::llm::error_classifier::*` | `openai/mod.rs` L24, `openai_compat.rs` L32 | 已在 `loom-llm/src/support/error_classifier/` | 🟢 |
| 15 | `crate::provider::load_provider_configs()` | `factory.rs` L22 | **留在 loom**（见 §3.4） | 🟢 |
| 16 | `crate::tier::plan::tier_plans()` | `factory.rs` L37 | **留在 loom**（见 §3.4） | 🟢 |
| 17 | `crate::test_util::*` | `openai/tests.rs` L1365 | 移除或改为 loom-llm 内的测试工具 | 🟡 |

### 3.2 ToolSource 依赖解法

`ChatOpenAI` 和 `ChatOpenAICompat` 各有一个 `new_with_tool_source()` 方法：

```rust
// 当前 loom 版（openai/mod.rs L123）
pub async fn new_with_tool_source(
    model: &str,
    tool_source: &dyn ToolSource,
) -> Result<Self, ToolSourceError> {
    let tools = tool_source.list_tools().await?;
    // ... 构建 client，存储 tools: Vec<ToolSpec>
}
```

**解法**：迁入 loom-llm 时，`new_with_tool_source()` 改为 `with_tools(Vec<ToolSpec>)` builder 方法。原有的 `new_with_tool_source()` 保留在 loom 侧作为便利函数：

```rust
// loom-llm 中（迁移后）
impl ChatOpenAI {
    /// 构建 client 并注入工具列表
    pub fn with_tools(mut self, tools: Vec<ToolSpec>) -> Self {
        self.tools = Some(tools);
        self
    }
}

// loom 中（扩展方法或便利函数）
pub async fn chat_openai_with_tools(
    model: &str,
    tool_source: &dyn ToolSource,
) -> Result<ChatOpenAI, ToolSourceError> {
    let tools = tool_source.list_tools().await?;
    Ok(ChatOpenAI::new(model).with_tools(tools))
}
```

**影响范围**：检查所有 `new_with_tool_source` 调用点：

```
grep -rn 'new_with_tool_source' loom/src/
```

实际调用点极少（或无），因为 ThinkNode 通常通过 `create_llm_client` + `with_tools` 方式构建。如有需要，在 loom 侧提供便利函数即可。

### 3.3 audit.rs 迁入

`audit.rs`（608 行）的 loom 依赖仅有一处：

```rust
use crate::memory::uuid6;  // → 改为 crate::support::uuid6（已在 loom-llm）
```

其余依赖全是标准库 + `serde` + `tokio`。迁移路径：

```
loom/src/llm/audit.rs → loom-llm/src/support/audit.rs
```

**改动**：替换 `use crate::memory::uuid6` → `use crate::support::uuid6`。

### 3.4 factory.rs 归属

`factory.rs`（69 行）依赖两个 loom 胶水模块：

```rust
use crate::provider::load_provider_configs();  // 从配置文件加载 provider
use crate::tier::plan::tier_plans();           // 获取 tier 方案
```

**方案**：`LlmFactory` 保留在 `loom/src/llm/factory.rs`。它是唯一留在 loom 的模块。

如果将来需要迁入 loom-llm，可改为构造时注入：
```rust
let factory = LlmFactory::new(providers, tier_plans);
```

但当前阶段保持简单，留在 loom 即可。

---

## 4. loom-llm 目录结构（迁移后）

```
loom-llm/
├── Cargo.toml
└── src/
    ├── lib.rs                              # crate root + 全量 re-export
    │
    ├── error.rs                            # AgentError, Interrupt ✅ 已有
    ├── message.rs                          # Message, UserContent, ... ✅ 已有
    ├── tool.rs                             # ToolCall, ToolSpec, ToolSourceError ✅ 已有
    ├── traits.rs                           # LlmClient, LlmProvider, LlmResponse ✅ 已有
    │
    ├── client/
    │   ├── mod.rs                          ✅ 已有（更新 mod 声明）
    │   ├── openai/                         ← 新增（从 loom 迁入）
    │   │   ├── mod.rs                      # ChatOpenAI（完整版 604 行）
    │   │   ├── models.rs                   # 模型列表（105 行）
    │   │   ├── request.rs                  # 请求构建（377 行）
    │   │   ├── stream.rs                   # 流式解析（432 行）
    │   │   └── tests.rs                    # 测试（442 行）
    │   ├── openai_compat.rs                ← 替换（342 行骨架 → 1,573 行完整版）
    │   ├── openai_provider.rs              ← 新增（从 loom 迁入，61 行）
    │   ├── openai_compat_provider.rs       ← 新增（从 loom 迁入，61 行）
    │   ├── retry.rs                        ✅ 已有
    │   ├── mock.rs                         ✅ 已有
    │   └── fixed_provider.rs               ✅ 已有
    │
    ├── support/
    │   ├── mod.rs                          ✅ 已有
    │   ├── audit.rs                        ← 新增（从 loom 迁入，608 行）
    │   ├── http_retry.rs                   ✅ 已有
    │   ├── uuid6.rs                        ✅ 已有
    │   ├── thinking.rs                     ✅ 已有
    │   ├── tool_call_accumulator.rs        ✅ 已有
    │   └── error_classifier/               ✅ 已有
    │
    └── registry.rs                         ← 扩展（加入 ModelRegistry 运行时，替换空壳）
```

### Cargo.toml 变更

```toml
# 新增依赖
model-spec-core = { path = "../model-spec-core" }   # model_registry.rs 需要

# 已有依赖保持不变
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
async-openai = { version = "0.32", features = ["chat-completion", "model"] }
sha2 = "0.10"         # uuid6 依赖（已添加）
mac_address = "1.1"   # uuid6 依赖（已添加）
```

---

## 5. loom crate 改动

### 5.1 `loom/src/llm/mod.rs` → 纯 re-export 薄壳

```rust
//! LLM module — re-exports from loom-llm crate.
//! All implementations live in loom-llm; this module preserves backward compatibility.

pub use loom_llm::{
    // Core types
    AgentError, Interrupt, GraphInterrupt,
    Message, UserContent, ContentPart, ContentError,
    AssistantPayload, AssistantToolCall, ToolCallContent,
    assistant_content_for_chat_api,
    ToolCall, ToolSpec, ToolSourceError,
    ToolOutputHint, ToolOutputStrategy,
    LlmClient, LlmProvider, LlmResponse, LlmUsage, LlmHeaders,
    ToolChoiceMode, ToolCallDelta, ModelInfo, ModelCapabilities,
    PromptTokensDetails, CompletionTokensDetails,
    MessageChunk, MessageChunkKind,

    // Clients
    ChatOpenAI, ChatOpenAICompat,
    OpenAIProvider, OpenAICompatProvider,
    RetryLlmClient, FixedLlmProvider,
    MockLlm, MultiRoundMockLlm,

    // Registry
    ProviderConfig, ModelEntry, ModelRegistry,
    CachedModelList, CombinedModelList,
    create_llm_client, create_llm_provider,

    // Support re-exports
    support::{audit, thinking, error_classifier},
};

/// LLM factory — stays in loom (depends on crate::provider and crate::tier).
mod factory;
pub use factory::LlmFactory;

#[deprecated(note = "renamed to ChatOpenAICompat")]
pub type ChatBigModel = ChatOpenAICompat;
```

### 5.2 删除的文件

```
删除:
  loom/src/llm/audit.rs
  loom/src/llm/model_registry.rs
  loom/src/llm/openai/                 (整个目录: mod.rs, models.rs, request.rs, stream.rs, tests.rs)
  loom/src/llm/openai_compat.rs
  loom/src/llm/openai_compat_provider.rs
  loom/src/llm/openai_provider.rs

保留:
  loom/src/llm/mod.rs                  (re-export 薄壳, ~40行)
  loom/src/llm/factory.rs              (依赖 crate::provider, crate::tier, 69行)
```

### 5.3 `loom/Cargo.toml` 可清理依赖

迁移完成后检查 `loom/Cargo.toml` 中 `async-openai` 是否还有其他模块使用。如果仅 `loom/src/llm/openai/` 使用，迁移后可从 loom 的直接依赖中移除（改为通过 `loom-llm` 间接依赖）。

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
  ├── pub use loom_llm::{Message, ToolCall, LlmClient, ...}        ← 类型 re-export
  ├── pub use loom_llm::{RetryLlmClient, MockLlm, ...}             ← 客户端 re-export
  ├── pub mod openai;                                               ← 实现在本地 (604行)
  ├── pub mod openai_compat;                                        ← 实现在本地 (1,573行)
  ├── pub mod model_registry;                                       ← 实现在本地 (574行)
  ├── pub mod audit;                                                ← 实现在本地 (608行)
  ├── pub mod openai_provider;                                      ← 实现在本地 (61行)
  └── pub mod openai_compat_provider;                               ← 实现在本地 (61行)

loom-llm/src/client/openai_compat.rs                                ← 简化版 (342行，无人使用)
loom-llm/src/registry.rs                                            ← 空壳类型 (320行)
```

### 6.2 迁移后

```
调用方 (loom/src/agent/react/build/llm.rs)
  │
  use crate::llm::{ChatOpenAI, create_llm_client, ...}            ← 路径不变
  │
  ▼
loom/src/llm/mod.rs
  ├── pub use loom_llm::{所有类型和实现}                             ← 纯 re-export (~40行)
  └── mod factory;                                                  ← 唯一本地模块 (69行)

loom-llm/src/
  ├── client/openai/mod.rs                                          ← 完整 ChatOpenAI (604行)
  ├── client/openai_compat.rs                                       ← 完整版 (1,573行)
  ├── client/openai_provider.rs                                     ← 完整版 (61行)
  ├── client/openai_compat_provider.rs                              ← 完整版 (61行)
  ├── support/audit.rs                                              ← 完整版 (608行)
  └── registry.rs                                                   ← 完整 ModelRegistry (574行)
```

---

## 7. Edge Cases

### 7.1 `ChatOpenAI` 的 `new_with_tool_source()` 方法

**场景**：`openai/mod.rs` 有 `new_with_tool_source(model, &dyn ToolSource)` 方法，直接调用 `tool_source.list_tools()`。

**处理**：
1. 迁入 loom-llm 时改为 `with_tools(tools: Vec<ToolSpec>)` builder 方法
2. loom 侧提供便利函数 `chat_openai_with_tools()` 封装 `ToolSource` 调用
3. 检查所有调用点，确保兼容

### 7.2 `ChatOpenAICompat` 的 `new_with_tool_source()` 方法

同 §7.1，处理方式一致。

### 7.3 `openai/tests.rs` 中的 `crate::test_util` 依赖

**场景**：`tests.rs` 引用 `crate::test_util::shared_client::test_client()`。

**处理**：
1. 首选：将测试迁入 `loom-llm`，替换为 loom-llm 内部的测试构建器
2. 备选：将 `tests.rs` 标记为 `#[cfg(test)]` 模块并保留在 loom 侧（但这会使 `ChatOpenAI` 测试与实现分离）
3. 推荐：检查测试内容，如果依赖 `test_util` 较深，考虑在迁移时简化或跳过这些测试

### 7.4 `loom-llm/src/client/openai_compat.rs` 双版本替换

**场景**：loom-llm 现有 342 行简化版 `ChatOpenAICompat`，需要替换为 loom 的 1,573 行完整版。

**处理**：
1. 先确认两个版本的公开 API 差异（`with_config`, `with_tools`, `with_headers` 等）
2. 完整版替换简化版，保持所有公开方法签名不变
3. 简化版的功能是完整版的子集，替换后行为兼容

### 7.5 `registry.rs` 双版本合并

**场景**：`loom-llm/src/registry.rs` 已有 `ProviderConfig`、`ModelEntry` 等数据类型（320行），`loom/src/llm/model_registry.rs` 有完整 `ModelRegistry` 运行时（574行）。

**处理**：
1. 数据类型已在 loom-llm（Phase 1 完成），不需要再迁
2. `ModelRegistry` struct、`create_llm_client()`、`create_llm_provider()` 函数迁入 loom-llm
3. `model_registry.rs` 中的 `CachedSpecProviders` 等内部类型一并迁入
4. `model_registry.rs` 顶部的 `pub use loom_llm::registry::{...}` re-export 改为内部 import

### 7.6 `stream-event` 依赖

`stream-event` 通过 `loom-llm` 的 `MessageChunk` 消费类型。迁移后类型仍在 `loom-llm`，`stream-event` 无需改动。确保 `loom-llm` 不反向依赖 `stream-event`。

---

## 8. 性能影响

| 维度 | 影响 |
|------|------|
| **编译时间** | `loom-llm` 增加 `model-spec-core` 依赖。纯数据 crate，编译增量 < 3s |
| **运行时** | 零影响。代码只是换了位置，逻辑不变 |
| **二进制大小** | 不变。loom 原本就通过 loom-llm 间接依赖这些 crate |
| **编译并行度** | **提升**。修改 `loom/src/agent/` 不再触发 `loom-llm` 重编译 |

---

## 9. 实施步骤

### Phase A：迁入 `audit.rs`（最低风险）

| 步骤 | 操作 | 验证 |
|------|------|------|
| A1 | 复制 `loom/src/llm/audit.rs` → `loom-llm/src/support/audit.rs` | — |
| A2 | 替换 `use crate::memory::uuid6` → `use crate::support::uuid6` | — |
| A3 | `loom-llm/src/support/mod.rs` 添加 `pub mod audit;` | — |
| A4 | `loom-llm/src/lib.rs` 添加 audit re-export | — |
| A5 | `loom/src/llm/mod.rs` 将 `pub mod audit;` 改为 `pub use loom_llm::support::audit;` | — |
| A6 | 删除 `loom/src/llm/audit.rs` | `cargo check --workspace` ✅ |

**风险**：🟢 低 — 仅 1 处 import 替换，无逻辑变更。

### Phase B：替换 `openai_compat.rs`（核心变更）

| 步骤 | 操作 | 改动点 |
|------|------|--------|
| B1 | 删除 `loom-llm/src/client/openai_compat.rs`（342行简化版） | — |
| B2 | 复制 `loom/src/llm/openai_compat.rs`（1,573行完整版）→ `loom-llm/src/client/openai_compat.rs` | — |
| B3 | 路径替换清单（见下表） | — |
| B4 | 处理 `ToolSource` 依赖（§3.2） | `new_with_tool_source` → `with_tools` |
| B5 | 删除 `loom/src/llm/openai_compat.rs` | `cargo check --workspace` ✅ |

**B3 路径替换表**：

| 原 loom 路径 | 替换为 loom-llm 路径 |
|-------------|---------------------|
| `crate::error::AgentError` | `crate::error::AgentError` |
| `crate::http_retry::*` | `crate::support::http_retry::*` |
| `crate::llm::error_classifier::*` | `crate::support::error_classifier::*` |
| `crate::llm::{LlmClient, LlmResponse, ...}` | `crate::traits::{LlmClient, LlmResponse, ...}` |
| `crate::memory::uuid6` | `crate::support::uuid6` |
| `crate::message::{Message, ...}` | `crate::message::{Message, ...}` |
| `crate::state::ToolCall` | `crate::tool::ToolCall` |
| `crate::stream::MessageChunk` | `crate::traits::MessageChunk` |
| `crate::tool_source::{ToolSource, ToolSourceError, ToolSpec}` | `crate::tool::{ToolSpec, ToolSourceError}` |

**风险**：🟡 中 — 文件最大（1,573行），但所有改动都是 import 路径替换。

### Phase C：迁入 `openai/` 子模块（ChatOpenAI）

| 步骤 | 操作 | 改动点 |
|------|------|--------|
| C1 | 复制 `loom/src/llm/openai/` 整目录 → `loom-llm/src/client/openai/` | 5 个文件 |
| C2 | 路径替换（同 B3 的替换表） | 各文件 import 区域 |
| C3 | `openai/mod.rs` 中 `new_with_tool_source` → `with_tools` builder | §3.2 |
| C4 | `openai/stream.rs` 路径替换 | `crate::llm::thinking` → `crate::support::thinking` 等 |
| C5 | `openai/request.rs` 路径替换 | `crate::tool_source::ToolSpec` → `crate::tool::ToolSpec` |
| C6 | `openai/tests.rs` 检查并适配或暂时 `#[cfg(skip)]` | §7.3 |
| C7 | `loom-llm/src/client/mod.rs` 添加 `pub mod openai;` | — |
| C8 | 删除 `loom/src/llm/openai/` | `cargo check --workspace` ✅ |

**风险**：🟡 中 — 5 个文件，但 `mod.rs` 和 `stream.rs` 路径替换较多。`tests.rs` 可能需要特殊处理。

### Phase D：迁入 Provider 实现

| 步骤 | 操作 |
|------|------|
| D1 | `loom/src/llm/openai_provider.rs` → `loom-llm/src/client/openai_provider.rs`（路径替换） |
| D2 | `loom/src/llm/openai_compat_provider.rs` → `loom-llm/src/client/openai_compat_provider.rs`（路径替换） |
| D3 | `loom-llm/src/client/mod.rs` 添加两个 mod 声明 |
| D4 | 删除 loom 侧两个文件 | `cargo check --workspace` ✅ |

**风险**：🟢 低 — 两个文件各 61 行，改动极小。

### Phase E：迁入 `model_registry.rs` 运行时

| 步骤 | 操作 | 改动点 |
|------|------|--------|
| E1 | `loom-llm/Cargo.toml` 添加 `model-spec-core = { path = "../model-spec-core" }` | — |
| E2 | 将 `ModelRegistry` struct + `create_llm_client` + `create_llm_provider` 从 `loom/src/llm/model_registry.rs` 追加到 `loom-llm/src/registry.rs` | — |
| E3 | 路径替换：`crate::model_spec::Provider` → `model_spec_core::spec::Provider as SpecProvider` | — |
| E4 | 路径替换：`crate::llm::{ChatOpenAI, ChatOpenAICompat}` → `crate::client::{ChatOpenAI, ChatOpenAICompat}` | — |
| E5 | 路径替换：`crate::error::AgentError` → `crate::error::AgentError` | — |
| E6 | 删除 `loom/src/llm/model_registry.rs` | `cargo check --workspace` ✅ |

**风险**：🟡 中 — 574 行，依赖替换路径明确。`model-spec-core` 是 workspace 内纯数据 crate，无风险。

### Phase F：`loom/src/llm/mod.rs` → 薄壳 + 全量验证

| 步骤 | 操作 |
|------|------|
| F1 | 重写 `loom/src/llm/mod.rs` 为 §5.1 所示的 re-export 薄壳 |
| F2 | 确认仅保留 `factory.rs`（唯一本地模块） |
| F3 | `cargo check --workspace` 全量编译 |
| F4 | `cargo test --workspace` 全量测试 |
| F5 | 检查 `loom/Cargo.toml` 是否可移除 `async-openai` 直接依赖 |
| F6 | 更新 `loom-llm/src/lib.rs` re-export 列表 |

**风险**：🟡 中 — 删除文件可能遗漏某些 `use crate::llm::*` 引用，编译器会报出具体错误。

---

## 10. 收益

| 维度 | 迁移前 | 迁移后 |
|------|--------|--------|
| **`loom/src/llm` 行数** | 4,987 | ~110（re-export + factory） |
| **`loom-llm/src` 行数** | 4,526（含简化版/空壳） | ~9,500（完整实现） |
| **loom-llm 独立可用** | ❌ 缺客户端、Registry、Audit | ✅ 完整 LLM crate |
| **编译隔离** | 改 agent 触发 llm 重编译 | 改 agent 不触发 loom-llm 重编译 |
| **依赖清晰度** | 不明（model-spec-core 在 loom） | 明确：reqwest + async-openai + model-spec-core |
| **可复用性** | 只能通过 loom 使用 | 第三方可直接 `loom-llm = "..."` |
| **loom crate 编译时间** | 含 ~5,000 行 LLM 实现 | 减少 ~5,000 行，编译加速 |

### 各 Phase 预计工作量

| Phase | 内容 | 行数变动 | 预计耗时 | 风险 |
|-------|------|---------|---------|------|
| A | audit.rs 迁入 | +608 | 30 min | 🟢 |
| B | openai_compat.rs 替换 | +1,231 净增 | 2-3 h | 🟡 |
| C | openai/ 子模块迁入 | +1,560 | 1-2 h | 🟡 |
| D | Provider 迁入 | +122 | 30 min | 🟢 |
| E | model_registry 迁入 | +574 | 1 h | 🟡 |
| F | 薄壳 + 验证 | -4,877 | 1-2 h | 🟡 |
| **总计** | | **~4,900 行** | **6-9 h** | |

---

## 11. 开发记录

### Phase A：audit.rs 迁入 ✅

**日期**：2026-06-03（前次迭代完成）

| 步骤 | 操作 | 状态 |
|------|------|------|
| A1 | 复制 `loom/src/llm/audit.rs` → `loom-llm/src/support/audit.rs` | ✅ |
| A2 | 替换 `use crate::memory::uuid6` → `use crate::support::uuid6` | ✅ |
| A3 | `loom-llm/src/support/mod.rs` 添加 `pub mod audit;` | ✅ |
| A4 | `loom-llm/src/lib.rs` 添加 audit re-export | ✅ |
| A5 | `loom/src/llm/mod.rs` 改为 `pub use loom_llm::support::audit;` | ✅ |
| A6 | 删除 `loom/src/llm/audit.rs` | ✅ |

**验证**：`cargo check --workspace` ✅

### Phase B：openai_compat.rs 替换 ✅

**日期**：2026-06-03（前次迭代完成文件复制，本次迭代完成切换）

文件 `loom-llm/src/client/openai_compat.rs` 已存在（1,559 行完整版，从 loom 342 行骨架替换而来）。

| 步骤 | 操作 | 状态 |
|------|------|------|
| B1 | 删除旧 342 行骨架版 | ✅（前次完成） |
| B2 | 复制 loom 完整版 → loom-llm，路径替换完成 | ✅（前次完成） |
| B3 | `new_with_tool_source` → 移除（无调用点） | ✅ |
| B4 | loom/src/llm/mod.rs 改为 `pub use loom_llm::client::ChatOpenAICompat` | ✅（本次） |
| B5 | 删除 `loom/src/llm/openai_compat.rs` | ✅（本次） |

**验证**：`cargo check --workspace` ✅

### Phase C：openai/ 子模块迁入 ✅

**日期**：2026-06-03（前次迭代完成文件复制，本次迭代完成切换）

文件已存在于 `loom-llm/src/client/openai/`（5 个文件：mod.rs 588行, models.rs 105行, request.rs 377行, stream.rs 432行, tests.rs 442行）。

| 步骤 | 操作 | 状态 |
|------|------|------|
| C1 | 复制文件到 loom-llm + 路径替换 | ✅（前次完成） |
| C2 | stream.rs 测试中 `crate::llm::thinking` → `crate::support::thinking` | ✅（本次修复） |
| C3 | loom/src/llm/mod.rs 改为 `pub use loom_llm::client::ChatOpenAI` | ✅（本次） |
| C4 | 删除 `loom/src/llm/openai/` 目录 | ✅（本次） |

**验证**：`cargo test -p loom-llm` → 163 tests passed ✅

### Phase D：Provider 迁入 ✅

**日期**：2026-06-03

| 步骤 | 操作 | 状态 |
|------|------|------|
| D1 | 创建 `loom-llm/src/client/openai_provider.rs`（61行，import 改为 `crate::`） | ✅ |
| D2 | 创建 `loom-llm/src/client/openai_compat_provider.rs`（61行，import 改为 `crate::`） | ✅ |
| D3 | `loom-llm/src/client/mod.rs` 添加两个 mod 声明 + pub use | ✅ |
| D4 | `loom-llm/src/lib.rs` 添加 provider re-export | ✅ |
| D5 | loom/src/llm/mod.rs 改为 `pub use loom_llm::client::{OpenAIProvider, OpenAICompatProvider}` | ✅ |
| D6 | 删除 loom 侧 `openai_provider.rs` + `openai_compat_provider.rs` | ✅ |
| D7 | `model_registry.rs` 中 `crate::llm::openai_provider::OpenAIProvider` → `crate::llm::OpenAIProvider` | ✅ |

**验证**：`cargo check --workspace` ✅

### Phase E：model_registry 处理决策 ✅

**日期**：2026-06-03

**决策**：`model_registry.rs` 保留在 `loom/src/llm/` 中，不迁入 `loom-llm`。

**原因**：
1. `ModelRegistry::fetch_or_get_cached_spec_providers()` 调用 `crate::model_spec::ModelsDevResolver`（定义在 `loom/src/model_spec/models_dev.rs`，不在 `model-spec-core` crate 中）
2. `create_llm_client()` / `create_llm_provider()` 使用 `ChatOpenAI`/`ChatOpenAICompat`（现在来自 `loom-llm`）和 `OpenAIConfig`（来自 `async-openai`）
3. 数据类型（`ProviderConfig`, `ModelEntry` 等）已通过 re-export 从 `loom-llm` 获取
4. 迁移 `ModelsDevResolver` 到 `loom-llm` 需要额外依赖解析（它依赖 `crate::http_retry` + `model_spec_core`），工作量与收益不成正比

**当前 `model_registry.rs` 的 loom 依赖**：
- `crate::error::AgentError` → re-export from `loom-llm` ✅
- `crate::llm::{ChatOpenAI, ChatOpenAICompat, LlmClient, LlmProvider}` → re-export from `loom-llm` ✅
- `crate::model_spec::Provider` → from `model-spec-core` ✅
- `crate::model_spec::ModelsDevResolver` → **loom only** (not in `model-spec-core`)

### Phase F：薄壳 + 全量验证 ✅

**日期**：2026-06-03

| 步骤 | 操作 | 状态 |
|------|------|------|
| F1 | 重写 `loom/src/llm/mod.rs` 为 re-export 薄壳（仅保留 `model_registry` + `factory` 本地模块） | ✅ |
| F2 | `cargo check --workspace` | ✅ |
| F3 | `cargo test -p loom-llm` → 163 passed ✅ | ✅ |
| F4 | `cargo test -p loom --lib llm` → 24 passed ✅ | ✅ |
| F5 | `cargo test -p loom --lib model_registry` → 3 passed ✅ | ✅ |
| F6 | `async-openai` 不可从 loom/Cargo.toml 移除（其他模块使用） | ✅ 确认 |

**最终 `loom/src/llm/` 结构**：
```
loom/src/llm/
├── mod.rs              (~73 行，纯 re-export + factory + model_registry)
├── model_registry.rs   (~575 行，运行时，依赖 ModelsDevResolver)
└── factory.rs          (~69 行，依赖 crate::provider/tier)
```

**对比**：
| 维度 | 迁移前 | 迁移后 |
|------|--------|--------|
| `loom/src/llm/` 文件数 | 12 个文件（含 openai/ 子目录） | 3 个文件 |
| `loom/src/llm/` 总行数 | ~4,987 行 | ~717 行 |
| `loom-llm/src/` 总行数 | ~4,526 行 | ~5,100+ 行 |
| 客户端实现位置 | loom 本地模块 | loom-llm crate |
| Provider 实现位置 | loom 本地模块 | loom-llm crate |
| 审计日志位置 | loom 本地模块 | loom-llm crate |

**编译验证**：`cargo check --workspace` ✅（仅有与迁移无关的 2 个 warning）

### 全量验证结果

| 测试 | 结果 |
|------|------|
| `cargo check --workspace` | ✅ 成功 |
| `cargo test -p loom-llm` | ✅ 163 passed, 0 failed |
| `cargo test -p loom --lib llm` | ✅ 24 passed, 0 failed |
| `cargo test -p loom --lib model_registry` | ✅ 3 passed, 0 failed |

注：`cargo test -p loom --lib` 有 8 个 pre-existing 失败（`stream_display::tool_preview` 4 个, `background_review::skill_registry` 2 个, `worktree::git_ops` 1 个），均与 LLM 迁移无关。
