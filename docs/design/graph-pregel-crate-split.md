# loom-graph + loom-pregel Crate 拆分方案

## 目标

将 `loom` crate 中的 `graph/`、`channels/`、`managed/`、`pregel/` 模块拆分为两个独立的 crate，实现清晰的分层架构：

- **`loom-graph`** — StateGraph 图执行引擎 + channels 聚合策略 + managed 运行时值 + memory 核心类型
- **`loom-pregel`** — Pregel-style BSP 批量同步并行运行时

拆分后 `loom` 通过 `pub use` 重导出，保持 `loom::graph::*` 和 `loom::pregel::*` 路径完全兼容。

---

## 1. 现状分析

### 1.1 代码量

| 模块 | 行数 | 文件数 |
|---|---|---|
| `graph/` | 6,086 | 16 |
| `channels/` | 1,498 | 8 |
| `managed/` | 98 | 1 |
| **graph 合计** | **7,682** | **25** |
| `pregel/` | 10,512 | 14 |

### 1.2 当前依赖关系

```
cli_run ──────────────┐
   │ (RunCancellation,│
   │  AnyStreamEvent, │
   │  ActiveOpKind)   │
   ▼                  ▼
error ◄── graph ◄─── channels       全部在 loom crate 内部
   │    (6.1K)       (1.5K)
   │      │
   │      ├── managed (98行)
   │      │
   │      └───► memory (Checkpoint, Checkpointer, RunnableConfig, Store)
   │      └───► stream (StreamEvent, StreamMode, StreamWriter)
   │
   └──── pregel ───► graph (RetryPolicy, Interrupt)
           (10.5K)  └──► memory, stream, cli_run
```

### 1.3 外部消费者

| 下游 crate | 引用 graph | 引用 pregel |
|---|---|---|
| `cli/` | ❌ 不直接引用 | ❌ |
| `serve/` | ❌ | ❌ |
| `loom-acp/` | ❌ | ❌ |
| `loom` crate 内部 `agent/` | ✅ 大量引用 | ❌ |
| `loom` crate 内部 `compress/` | ✅ | ❌ |

Pregel 和 Graph 的外部消费者都仅限于 `loom` crate 内部。这使拆分的影响范围可控。

---

## 2. 拆分目标架构

### 2.1 目标依赖图

```
  loom-llm          stream-event
  (AgentError,       (StreamEvent,
   Interrupt)         StreamMode)
     │                   │
     │      ┌────────────┤
     ▼      ▼            ▼
  ┌──────────────────────────────────┐
  │         loom-graph               │
  │  StateGraph, CompiledStateGraph, │
  │  Node, Next, RetryPolicy,       │
  │  channels, managed,             │
  │  Checkpoint<V>, Checkpointer,   │
  │  RunnableConfig, Store          │
  │  (7.7K lines)                   │
  └──────────┬───────────────────────┘
             │
             ▼
  ┌──────────────────────────────────┐
  │         loom-pregel              │
  │  PregelRuntime, PregelGraph,    │
  │  PregelLoop, PregelRunner,      │
  │  Checkpoint<V> consumer,        │
  │  Checkpointer consumer          │
  │  (10.5K lines)                  │
  └──────────┬───────────────────────┘
             │
             ▼
  ┌──────────────────────────────────┐
  │            loom                  │
  │  cli_run, agent (ReAct/ToT/...),│
  │  tools, memory impl,            │
  │  tool_source, compress, etc.    │
  │                                  │
  │  pub use loom_graph as graph;   │
  │  pub use loom_pregel as pregel; │
  └──────────────────────────────────┘
```

### 2.2 依赖方向规则

- `loom-pregel` → `loom-graph` → `loom-llm`
- `loom-pregel` → `stream-event`
- `loom` → `loom-pregel` + `loom-graph`
- **无环**：`loom-graph` 不依赖 `loom`，`loom-pregel` 不依赖 `loom`

---

## 3. loom-graph Crate 详情

### 3.1 目录结构

```
loom-graph/
├── Cargo.toml
└── src/
    ├── lib.rs                     # 公共 API + 重导出
    ├── memory.rs                  # Checkpoint<V>, Checkpointer trait, RunnableConfig, Store trait
    │
    ├── node.rs                    # Node trait
    ├── next.rs                    # Next enum
    ├── retry.rs                   # RetryPolicy
    ├── compile_error.rs           # CompilationError
    ├── conditional.rs             # ConditionalRouter, ConditionalRouterFn, NextEntry
    ├── interrupt.rs               # InterruptHandler trait, DefaultInterruptHandler
    ├── name_node.rs               # NameNode
    ├── node_middleware.rs         # NodeMiddleware trait
    ├── logging.rs                 # logging 辅助函数
    ├── logging_middleware.rs      # LoggingNodeMiddleware
    ├── visualization.rs           # generate_dot, generate_text
    ├── runtime.rs                 # Runtime<C, S>
    ├── run_context.rs             # RunContext<S> (解耦后)
    ├── compiled.rs                # CompiledStateGraph<S> (解耦后)
    ├── state_graph.rs             # StateGraph<S>, START, END
    ├── cancellable.rs             # run_cancellable (解耦后)
    │
    ├── channels/
    │   ├── mod.rs                 # Channel trait + 重导出
    │   ├── binop.rs               # BinaryOperatorAggregate
    │   ├── ephemeral_value.rs     # EphemeralValue
    │   ├── error.rs               # ChannelError
    │   ├── last_value.rs          # LastValue
    │   ├── named_barrier.rs       # NamedBarrierValue
    │   ├── topic.rs               # Topic
    │   └── updater.rs             # StateUpdater, ReplaceUpdater, FieldBasedUpdater
    │
    └── managed.rs                 # ManagedValue trait, IsLastStep
```

### 3.2 Cargo.toml

```toml
[package]
name = "loom-graph"
version.workspace = true
edition.workspace = true
description = "StateGraph, channels, and graph execution primitives for Loom"
license.workspace = true

[dependencies]
# 核心依赖
loom-llm = { path = "../loom-llm" }
stream-event = { path = "../stream-event" }

# 异步运行时
tokio = { workspace = true }
tokio-util = { version = "0.7", features = ["rt"] }
async-trait = { workspace = true }
futures-util = "0.3"

# 序列化
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# 错误处理
thiserror = { workspace = true }

# 日志
tracing = "0.1"
```

### 3.3 对外依赖（仅 3 个）

| 依赖 | 提供的类型 | 用途 |
|---|---|---|
| `loom-llm` | `AgentError`, `Interrupt`, `GraphInterrupt` | 错误处理和中断 |
| `stream-event` | `StreamEvent`, `StreamMode`, `StreamWriter`, `MessageChunk`, `StreamMetadata` | 流式输出 |
| `tokio-util` | `CancellationToken` | 取消机制 |

**不依赖**：`rusqlite`, `lancedb`, `mcp_client`, `dashmap`, `reqwest`, `crossbeam-channel`

### 3.4 memory 核心类型定义

在 `loom-graph/src/memory.rs` 中定义被 graph 和 pregel 共用的核心类型：

```rust
//! Memory core types shared by graph and pregel crates.
//!
//! Trait definitions and basic data types for checkpointing and long-term storage.
//! Concrete implementations (MemorySaver, SqliteSaver, etc.) remain in the `loom` crate.

/// Per-run state snapshot for resume, replay, branching, and inspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint<V> {
    pub id: String,
    pub ts: String,
    pub channel_values: V,
    pub channel_versions: HashMap<String, String>,
    pub versions_seen: HashMap<String, HashMap<String, String>>,
    pub pending_sends: Vec<PendingWrite>,
    pub pending_writes: Vec<PendingWrite>,
    pub pending_interrupts: Vec<serde_json::Value>,
    pub updated_channels: Option<Vec<String>>,
    pub kernel: CheckpointKernel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointKernel {
    pub step: i64,
    pub parents: HashMap<String, String>,
    pub children: HashMap<String, Vec<String>>,
}

/// Why the checkpoint was created.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CheckpointSource {
    Input,
    Loop,
    Resume,
}

/// Trait for checkpoint persistence backends.
#[async_trait]
pub trait Checkpointer<V>: Send + Sync {
    async fn put(&self, config: &RunnableConfig, checkpoint: Checkpoint<V>) -> Result<(), CheckpointError>;
    async fn get(&self, config: &RunnableConfig) -> Result<Option<Checkpoint<V>>, CheckpointError>;
    // ... 列出、删除等方法
}

/// Runtime configuration passed to graph/pregel execution methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnableConfig {
    pub thread_id: String,
    pub checkpoint_id: Option<String>,
    pub checkpoint_ns: String,
    pub user_id: Option<String>,
    // ...
}

/// Cross-session key-value storage trait.
#[async_trait]
pub trait Store: Send + Sync {
    async fn get(&self, namespace: &[String], key: &str) -> Result<Option<serde_json::Value>, StoreError>;
    async fn put(&self, namespace: &[String], key: &str, value: serde_json::Value) -> Result<(), StoreError>;
    async fn delete(&self, namespace: &[String], key: &str) -> Result<(), StoreError>;
    async fn search(&self, namespace: &[String], query: &str, limit: usize) -> Result<Vec<...>, StoreError>;
}
```

---

## 4. loom-pregel Crate 详情

### 4.1 目录结构

```
loom-pregel/
├── Cargo.toml
└── src/
    ├── lib.rs                     # 公共 API + 重导出
    ├── types.rs                   # ChannelName, NodeName, TaskId, ReservedWrite, LoopStatus...
    ├── channel.rs                 # Channel trait, LastValueChannel, TopicChannel, etc.
    ├── cache.rs                   # PregelTaskCache trait, InMemoryPregelTaskCache
    ├── config.rs                  # PregelConfig, PregelDurability
    ├── node.rs                    # PregelGraph, PregelNode, PregelNodeInput/Output/Context
    ├── algo.rs                    # prepare_next_tasks, apply_writes, etc.
    ├── loop_state.rs              # PregelLoop 状态机
    ├── runner.rs                  # PregelRunner
    ├── runtime.rs                 # PregelRuntime (主入口)
    ├── graph_view.rs              # PregelGraphView (导出/可视化)
    ├── validate.rs                # 静态图验证
    ├── state.rs                   # PregelStateSnapshot, StateUpdateRequest
    ├── subgraph.rs                # SubgraphInvocation, PregelSubgraph
    └── replay.rs                  # ReplayMode, ReplayRequest, ReplayResult
```

### 4.2 Cargo.toml

```toml
[package]
name = "loom-pregel"
version.workspace = true
edition.workspace = true
description = "Pregel-style BSP graph runtime for Loom"
license.workspace = true

[dependencies]
# 核心依赖
loom-llm = { path = "../loom-llm" }
loom-graph = { path = "../loom-graph" }
stream-event = { path = "../stream-event" }

# 异步运行时
tokio = { workspace = true }
tokio-util = { version = "0.7", features = ["rt"] }
tokio-stream = { workspace = true }
async-trait = { workspace = true }

# 序列化
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# 并发
dashmap = "6.0"

# 日志
tracing = "0.1"
```

### 4.3 对 loom-graph 的依赖（仅 5 个类型）

| 类型 | 来源 | 引用文件 |
|---|---|---|
| `RetryPolicy` | `loom_graph::RetryPolicy` | config.rs, runner.rs |
| `Interrupt` | `loom_llm::error::Interrupt` (通过 loom-graph 重导出) | algo.rs, loop_state.rs, node.rs |
| `GraphInterrupt` | `loom_llm::error::GraphInterrupt` (通过 loom-graph 重导出) | node.rs, runtime.rs (tests) |
| `Checkpoint<V>` | `loom_graph::memory::Checkpoint` | algo.rs, loop_state.rs, runtime.rs, state.rs |
| `Checkpointer` | `loom_graph::memory::Checkpointer` | runtime.rs |
| `RunnableConfig` | `loom_graph::memory::RunnableConfig` | algo.rs, node.rs, runtime.rs |
| `Store` | `loom_graph::memory::Store` | runtime.rs |
| `StreamEvent` | `stream_event::StreamEvent` | node.rs, runner.rs, runtime.rs |
| `StreamMode` | `stream_event::StreamMode` | config.rs, node.rs, runner.rs, runtime.rs |

---

## 5. 解耦方案

### 5.1 RunCancellation → CancellationToken

**问题**：`graph/cancellable.rs`、`graph/compiled.rs`、`graph/run_context.rs` 以及 `pregel/node.rs`、`pregel/loop_state.rs`、`pregel/runtime.rs` 都引用 `cli_run::RunCancellation`。

**方案**：底层 crate 使用 `tokio_util::sync::CancellationToken`，`loom` 层做桥接。

```rust
// ─── loom-graph: compiled.rs ───
use tokio_util::sync::CancellationToken;

impl<S> CompiledStateGraph<S> {
    pub fn with_cancellation_token(mut self, token: CancellationToken) -> Self {
        self.cancellation_token = Some(token);
        self
    }
}

// ─── loom: cli_run/agent.rs (桥接) ───
impl RunCancellation {
    pub fn to_cancellation_token(&self) -> CancellationToken {
        self.token.clone()
    }
}

// 调用处:
let compiled = graph.compile()
    .with_cancellation_token(run_cancellation.to_cancellation_token());
```

### 5.2 AnyStreamEvent → 泛型回调

**问题**：`graph/compiled.rs` 和 `graph/run_context.rs` 引用 `cli_run::AnyStreamEvent`。`AnyStreamEvent` 是一个包装了 `StreamEvent<S>` 的枚举，属于 CLI/ACP 层概念。

**方案**：`RunContext` 使用泛型事件回调，`loom` 层做类型转换。

```rust
// ─── loom-graph: run_context.rs ───
pub struct RunContext<S> {
    /// Optional callback for forwarding raw stream events to external consumers.
    pub event_forwarder: Option<Arc<dyn Fn(StreamEvent<S>) + Send + Sync>>,
    // ...
}

// ─── loom: cli_run/agent.rs (桥接) ───
let forwarder = Arc::new(|ev: StreamEvent<MyState>| {
    if let Some(sender) = &opts.any_stream_event_sender {
        sender(AnyStreamEvent::React(ev));
    }
});
let ctx = RunContext::new(config).with_event_forwarder(forwarder);
```

### 5.3 ActiveOperationKind → &str

**问题**：`graph/cancellable.rs` 引用 `cli_run::ActiveOperationKind` 用于注册 abortable 操作。

**方案**：将 abort handle 注册逻辑从 `run_cancellable` 中移出，`cancellable` 只保留纯取消逻辑。

```rust
// ─── loom-graph: cancellable.rs (简化) ───
pub async fn run_cancellable<T, E>(
    future: impl Future<Output = Result<T, E>>,
    cancellation: Option<&CancellationToken>,
) -> Result<Result<T, E>, AgentError> {
    // 只做: cancel token race + 10min fallback timeout
    // 不再管理 abort handle 注册
}

// ─── loom: 调用侧 ───
// abort handle 注册由 cli_run 层在外部处理
let (task, abort_handle) = abortable(future);
if let Some(rc) = &run_cancellation {
    rc.set_abortable_operation(op_kind, abort_handle);
}
let result = run_cancellable(task, cancellation_token.as_ref()).await;
```

### 5.4 memory 核心类型下沉

**问题**：`Checkpoint<V>`、`Checkpointer` trait、`RunnableConfig`、`Store` trait 被三个层（graph、pregel、loom）共用。

**方案**：类型和 trait 定义下沉到 `loom-graph/src/memory.rs`，具体实现留在 `loom` 中。

```
loom-graph/src/memory.rs     定义: Checkpoint<V>, CheckpointKernel, CheckpointSource,
                              Checkpointer<V> trait, RunnableConfig, Store trait,
                              CheckpointError, StoreError, PendingWrite

loom/src/memory/mod.rs        pub use loom_graph::memory::{...};
                              + 保留实现: MemorySaver, SqliteSaver, InMemoryStore,
                                SqliteStore, JsonSerializer, SqliteVecStore
```

`loom/src/memory/mod.rs` 改为：

```rust
//! Memory module: checkpointing and long-term store.
//!
//! Core types are defined in `loom_graph::memory`.
//! This module re-exports them and provides concrete implementations.

// Re-export core types from loom-graph
pub use loom_graph::memory::{
    Checkpoint, CheckpointError, CheckpointKernel, CheckpointSource, Checkpointer,
    PendingWrite, RunnableConfig, Store, StoreError,
};

// Concrete implementations (stay in loom)
mod memory_saver;
mod sqlite_saver;
mod in_memory_store;
mod sqlite_store;
mod json_serializer;
// ...

pub use memory_saver::MemorySaver;
pub use sqlite_saver::SqliteSaver;
pub use in_memory_store::InMemoryStore;
// ...
```

---

## 6. loom crate 改动

### 6.1 Cargo.toml

```toml
[dependencies]
# 新增
loom-graph = { path = "../loom-graph" }
loom-pregel = { path = "../loom-pregel" }

# 已有，保持不变
loom-llm = { path = "../loom-llm" }
stream-event = { path = "../stream-event" }
# ...
```

### 6.2 lib.rs

```rust
// 改前:
// pub mod channels;
// pub mod graph;
// pub mod managed;
// pub mod pregel;

// 改后:
pub use loom_graph::channels;   // loom::channels::* 路径不变
pub use loom_graph as graph;    // loom::graph::* 路径不变
pub use loom_graph::managed;    // loom::managed::* 路径不变
pub use loom_pregel as pregel;  // loom::pregel::* 路径不变

// pub use 重导出列表也需要更新:
pub use loom_graph::{
    CompiledStateGraph, CompilationError, ConditionalRouter, ConditionalRouterFn,
    DefaultInterruptHandler, GraphInterrupt, Interrupt, InterruptHandler, LoggingNodeMiddleware,
    NameNode, Next, NextEntry, Node, NodeMiddleware, RetryPolicy, RunContext, Runtime,
    StateGraph, END, START,
    generate_dot, generate_text, run_cancellable,
};
pub use loom_pregel::{
    // ... 所有 pregel 公共类型
};
```

### 6.3 删除目录

拆分完成后删除：
- `loom/src/graph/` （16 个文件）
- `loom/src/channels/` （8 个文件）
- `loom/src/managed/` （1 个文件）
- `loom/src/pregel/` （14 个文件）

### 6.4 memory 模块重构

`loom/src/memory/` 保留实现文件，顶部添加重导出（见 §5.4）。

---

## 7. 实施阶段

### Phase 1: loom-graph 骨架 + 无依赖文件

**目标**：创建 `loom-graph` crate，移入不依赖 `cli_run` 和 `memory` 的纯文件。

| 步骤 | 文件 | 改动 |
|---|---|---|
| 1.1 | 创建 `loom-graph/` 目录和 `Cargo.toml` | 新建 |
| 1.2 | `next.rs` | 原样移入 |
| 1.3 | `retry.rs` | 原样移入，改 `crate::graph::Next` 引用为 `super::Next` |
| 1.4 | `compile_error.rs` | 原样移入 |
| 1.5 | `conditional.rs` | 原样移入 |
| 1.6 | `channels/` 目录 | 整体移入（无外部依赖） |
| 1.7 | `interrupt.rs` | 改 `crate::error::AgentError` → `loom_llm::error::AgentError` |
| 1.8 | `name_node.rs` | 同上 |
| 1.9 | `node_middleware.rs` | 同上 |
| 1.10 | `logging.rs` | 同上 |
| 1.11 | `logging_middleware.rs` | 同上 |
| 1.12 | `visualization.rs` | 同上 |
| 1.13 | `lib.rs` | 汇总所有 `pub use` |

**验证**：`cargo build -p loom-graph` 通过。

**风险**：🟢 低 — 全部是纯值类型或只依赖 `loom-llm`。

### Phase 2: memory 核心类型下沉

**目标**：在 `loom-graph` 中定义 `Checkpoint<V>`、`Checkpointer` trait、`RunnableConfig`、`Store` trait。

| 步骤 | 文件 | 改动 |
|---|---|---|
| 2.1 | `loom-graph/src/memory.rs` | 新建，从 `loom/src/memory/` 提取类型和 trait |
| 2.2 | `loom/src/memory/mod.rs` | 添加 `pub use loom_graph::memory::*`，删除已移动的定义 |
| 2.3 | `loom-graph/src/lib.rs` | 添加 `pub mod memory;` |

**验证**：`cargo build -p loom-graph` + `cargo build -p loom` 通过。

**风险**：🟡 中 — `Checkpoint<V>` 被 `memory` 模块多处使用，需要仔细处理字段和方法完整性。

### Phase 3: graph 核心文件移入

**目标**：移入 `graph` 模块剩余的有依赖文件。

| 步骤 | 文件 | 改动 |
|---|---|---|
| 3.1 | `node.rs` | 改 `crate::error` → `loom_llm::error`，`crate::graph::RunContext` → `super::RunContext` |
| 3.2 | `runtime.rs` | 改 `crate::memory` → `super::memory`，`crate::stream` → `stream_event` |
| 3.3 | `managed.rs` | 改 `crate::graph::RunContext` → `crate::RunContext` |
| 3.4 | `run_context.rs` | **重点改写**：`AnyStreamEvent` → 泛型回调，`RunCancellation` → `CancellationToken`，`crate::managed::ManagedValue` → `crate::managed::ManagedValue`（已在 crate 内） |
| 3.5 | `cancellable.rs` | **重点改写**：移除 `RunCancellation` 和 `ActiveOperationKind`，只保留 `CancellationToken` |
| 3.6 | `compiled.rs` | **重点改写**：`RunCancellation` → `CancellationToken`，`AnyStreamEvent` → 泛型回调，`crate::memory` → `crate::memory`（已在 crate 内） |
| 3.7 | `state_graph.rs` | 改 `crate::channels` → `crate::channels`（已在 crate 内），`crate::memory` → `crate::memory` |

**验证**：`cargo build -p loom-graph` 通过。

**风险**：🟡 中 — `compiled.rs` 是最复杂的文件（2,091 行），改动较多。

### Phase 4: loom-graph 集成

**目标**：让 `loom` crate 使用 `loom-graph`。

| 步骤 | 内容 |
|---|---|
| 4.1 | `loom/Cargo.toml` 添加 `loom-graph = { path = "../loom-graph" }` |
| 4.2 | `loom/src/lib.rs`: `pub mod graph` → `pub use loom_graph as graph` |
| 4.3 | `loom/src/lib.rs`: `pub mod channels` → `pub use loom_graph::channels` |
| 4.4 | `loom/src/lib.rs`: `pub mod managed` → `pub use loom_graph::managed` |
| 4.5 | 删除 `loom/src/graph/`、`loom/src/channels/`、`loom/src/managed/` |
| 4.6 | `loom/src/cli_run/` 中添加 `RunCancellation → CancellationToken` 桥接 |
| 4.7 | 修复 `loom` 内部 `agent/`、`compress/` 等模块的编译错误 |

**验证**：`cargo build -p loom` + `cargo test -p loom` 通过。

**风险**：🟡 中 — `agent/` 下的 ReAct/ToT/GoT/DUP 模块大量使用 `crate::graph::*`，需要改为 `loom_graph::*` 或通过重导出路径。

### Phase 5: loom-pregel 骨架

**目标**：创建 `loom-pregel` crate，移入纯内部文件。

| 步骤 | 文件 | 改动 |
|---|---|---|
| 5.1 | 创建 `loom-pregel/` 目录和 `Cargo.toml` | 新建 |
| 5.2 | `types.rs` | 原样移入 |
| 5.3 | `channel.rs` | 改 `crate::pregel::types` → `crate::types` |
| 5.4 | `cache.rs` | 同上 |
| 5.5 | `replay.rs` | 同上 |
| 5.6 | `subgraph.rs` | 改 `crate::pregel::runtime` → `crate::runtime` |
| 5.7 | `config.rs` | 改 `crate::graph::RetryPolicy` → `loom_graph::RetryPolicy`，`crate::stream::StreamMode` → `stream_event::StreamMode` |
| 5.8 | `graph_view.rs` | 改 `crate::pregel::*` → `crate::*`，`crate::error` → `loom_llm::error` |
| 5.9 | `validate.rs` | 改 `crate::error` → `loom_llm::error` |

**验证**：`cargo build -p loom-pregel` 通过。

**风险**：🟢 低 — 主要是路径替换。

### Phase 6: loom-pregel 核心文件

**目标**：移入 pregel 模块的核心执行文件。

| 步骤 | 文件 | 改动 |
|---|---|---|
| 6.1 | `node.rs` | 改 `crate::cli_run::RunCancellation` → `CancellationToken`，`crate::graph::Interrupt` → `loom_graph::Interrupt`，`crate::memory` → `loom_graph::memory`，`crate::stream` → `stream_event` |
| 6.2 | `algo.rs` | 改 `crate::memory` → `loom_graph::memory` |
| 6.3 | `loop_state.rs` | 改 `crate::cli_run::RunCancellation` → `CancellationToken`，`crate::graph::Interrupt` → `loom_graph::Interrupt` |
| 6.4 | `runner.rs` | 改 `crate::error` → `loom_llm::error`，`crate::graph::RetryPolicy` → `loom_graph::RetryPolicy` |
| 6.5 | `runtime.rs` | **最大文件 (5,877 行)**，全面改写 `crate::` 引用 |
| 6.6 | `state.rs` | 改 `crate::memory::Checkpoint` → `loom_graph::memory::Checkpoint` |

**验证**：`cargo build -p loom-pregel` 通过。

**风险**：🟡 中 — `runtime.rs` 5,877 行是最大文件，测试代码中也引用了 `crate::graph` 和 `crate::memory`。

### Phase 7: loom 切换到 loom-pregel

| 步骤 | 内容 |
|---|---|
| 7.1 | `loom/Cargo.toml` 添加 `loom-pregel = { path = "../loom-pregel" }` |
| 7.2 | `loom/src/lib.rs`: `pub mod pregel` → `pub use loom_pregel as pregel` |
| 7.3 | 删除 `loom/src/pregel/` |
| 7.4 | 修复 `loom` 内部引用 `crate::pregel::*` 的编译错误 |

**验证**：`cargo build -p loom` 通过。

**风险**：🟢 低 — pregel 没有外部消费者。

### Phase 8: 全量验证

| 步骤 | 内容 |
|---|---|
| 8.1 | `cargo build --workspace` 全量编译 |
| 8.2 | `cargo test -p loom-graph` 独立测试 |
| 8.3 | `cargo test -p loom-pregel` 独立测试 |
| 8.4 | `cargo test -p loom` 集成测试 |
| 8.5 | `cargo test --workspace` 全量测试 |
| 8.6 | `cargo test -p cli` CLI 测试 |

**风险**：🟢 低 — 逐步验证，及时回滚。

---

## 8. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|---|---|---|
| `Checkpoint<V>` 字段不完整导致编译失败 | Phase 2 | 逐字段对比原定义，确保所有字段和方法都迁移 |
| `compiled.rs` 2,091 行改写引入 bug | Phase 3 | 改写后立即运行 `cargo test -p loom-graph` |
| `runtime.rs` 5,877 行路径替换遗漏 | Phase 6 | 用 `grep -rn 'crate::graph\|crate::memory\|crate::stream\|crate::cli_run\|crate::error'` 验证无遗漏 |
| `agent/` 模块引用 `crate::graph` 需更新 | Phase 4 | 通过 `loom` 的 `pub use loom_graph as graph` 重导出，大部分路径不需要改 |
| `RunContext` 的 `AnyStreamEvent` 改动影响 ACP | Phase 3 | 桥接层确保类型转换正确，ACP 不直接依赖 loom-graph |
| `managed.rs` 对 `RunContext` 的循环依赖 | Phase 3 | `managed.rs` 和 `RunContext` 都在 `loom-graph` 内部，使用 `super::` 引用 |

---

## 9. 收益

| 维度 | loom-graph | loom-pregel | 合计效果 |
|---|---|---|---|
| **独立编译** | 修改 tools/agent 不重编译 graph | 修改 graph 不重编译 pregel | 三层独立编译 |
| **依赖精简** | 只需 loom-llm + stream-event + tokio | 只需 loom-graph + loom-llm + stream-event | rusqlite/mcp_client 等不污染 |
| **可复用** | 第三方可直接用 StateGraph | 第三方可直接用 BSP runtime | 无需拉入 loom 全量 |
| **编译时间** | ~7.7K 行独立编译 | ~10.5K 行独立编译 | 改应用层不触发底层重编译 |
| **下游兼容** | `loom::graph::*` 路径不变 | `loom::pregel::*` 路径不变 | 零破坏性 |
| **测试隔离** | graph 单元测试独立运行 | pregel ~100 个测试独立运行 | 不需要 LLM/MCP 基础设施 |

---

## 10. 最终 Workspace 成员

```toml
[workspace]
members = [
    # 核心层 (无 loom 依赖)
    "loom-llm",         # LLM client + 错误类型
    "stream-event",     # 流事件类型
    "loom-graph",       # StateGraph + channels + managed + memory 核心   ← NEW
    "loom-pregel",      # Pregel BSP runtime                              ← NEW

    # 框架层
    "loom",             # Agent 框架 (depends on ↑)
    "config",
    "model-spec-core",
    "loom-workspace",
    "loom-skill",
    "loom-curator",
    "task-core",

    # 应用层
    "cli",
    "serve",
    "loom-acp",
    "task-cli",
    "task-mcp-server",
    "loom-examples",
    "telegram-bot",
]
```
