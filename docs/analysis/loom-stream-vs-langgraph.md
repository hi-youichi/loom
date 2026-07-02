# Loom-Stream vs LangGraph 架构对比与重构方案

> 日期: 2025-08-19
> 状态: 草案（基于 LangGraph 源码审查）
> 模块: foundation/graph-core, foundation/pregel, stream-event, loom-stream
> 源码版本: langgraph 2025-07 (main branch)

---

## 1. 架构对比总览

| 维度 | Loom (Rust) | LangGraph (Python) |
|------|-------------|-------------------|
| **执行模型** | 双轨：graph-core (顺序) + pregel (并行 BSP) | 单一 Pregel BSP 模型 |
| **状态表示** | `serde_json::Value` (类型擦除) | `TypedDict` + `Annotated[reducer]` (编译时注解) |
| **Channel 系统** | 两套独立实现 (graph-core vs pregel) | 统一 `BaseChannel` 抽象 |
| **Node 接口** | 两套：`Node<S>` vs `PregelNode` | 统一 `PregelNode` (继承 `Runnable`) |
| **Streaming** | `StreamEvent<S>` 泛型 + 类型擦除并存 | `StreamProtocol` + 多 mode (values/updates/messages/debug) |
| **Checkpoint** | 深度耦合到 Pregel，JSON-only | `BaseCheckpointSaver[V]` 泛型基类，可插拔 |
| **子图** | 手动 namespace 管理 | 自动 namespace + `checkpoint_ns` 层级 |
| **错误处理** | `GraphError::ExecutionFailed(String)` 信息丢失 | `GraphInterrupt` + `RetryPolicy` + `NodeError` |
| **类型安全** | 运行时 JSON 反序列化 | Python TypedDict + Annotated reducer |

---

## 2. LangGraph 核心架构（源码分析）

### 2.1 执行循环：PregelLoop

```python
# langgraph/pregel/_loop.py
class PregelLoop:
    config: RunnableConfig
    checkpointer: BaseCheckpointSaver | None
    channels: Mapping[str, BaseChannel]
    checkpoint: Checkpoint
    tasks: dict[str, PregelExecutableTask]
    stream: StreamProtocol | None
    
    def tick(self) -> bool:
        """执行一个 superstep"""
        # 1. 准备下一批 tasks
        self.tasks = prepare_next_tasks(...)
        # 2. 执行 tasks，收集 writes
        # 3. 应用 writes 到 channels
        apply_writes(...)
        # 4. 保存 checkpoint
        self._put_checkpoint(...)
        # 5. 检查是否应该中断
        if should_interrupt(...):
            raise GraphInterrupt(...)
```

**关键设计**：
- `PregelLoop` 是执行引擎的核心，管理整个执行生命周期
- `channels` 是 `Mapping[str, BaseChannel]`，每个 channel 有类型
- `checkpoint` 是 `Checkpoint` TypedDict，包含 `channel_values`, `channel_versions`, `versions_seen`

### 2.2 Channel 系统

```python
# langgraph/channels/base.py
class BaseChannel(Generic[V]):
    def update(self, values: Sequence[Any]) -> bool: ...
    def consume(self) -> bool: ...
    def is_available(self) -> bool: ...
    def is_empty(self) -> bool: ...
    def is_reset(self) -> bool: ...
    def is_checkpoint(self) -> bool: ...
    def is_checkpointable(self) -> bool: ...

# langgraph/channels/last_value.py
class LastValue(BaseChannel[V]):
    """只保留最后一个值的 channel"""
    def update(self, values: Sequence[Any]) -> bool:
        if len(values) == 0: return False
        if len(values) == 1:
            self.value = values[-1]
            return True
        raise InvalidUpdateError("LastValue channel only accepts one value")

# langgraph/channels/topic.py  
class Topic(BaseChannel[Sequence[V]]):
    """累积所有值的 channel，支持 reducer"""
    def __init__(self, reducer: Callable | None = None, ...):
        self.reducer = reducer or _default_reducer
        self.values = []
    
    def update(self, values: Sequence[Any]) -> bool:
        if self.reducer:
            self.values = [self.reducer(self.values + values)]
        else:
            self.values.extend(values)
        return True
```

**关键设计**：
- `BaseChannel` 是统一抽象，所有 channel 实现相同接口
- `LastValue` 只保留最后一个值（用于状态字段）
- `Topic` 累积值，支持 reducer（用于消息列表等）
- `BinaryOperatorAggregate` 支持自定义聚合函数

### 2.3 Checkpoint 系统

```python
# langgraph/checkpoint/base/__init__.py
class Checkpoint(TypedDict):
    v: int                          # 格式版本
    id: str                         # 唯一 ID（单调递增）
    ts: str                         # ISO 8601 时间戳
    channel_values: dict[str, Any]  # channel 快照值
    channel_versions: ChannelVersions  # channel 版本号
    versions_seen: dict[str, ChannelVersions]  # 每个 node 见过的版本
    updated_channels: list[str] | None  # 本次更新的 channels

class BaseCheckpointSaver(Generic[V]):
    """可插拔的 checkpoint 持久化接口"""
    serde: SerializerProtocol = JsonPlusSerializer()
    
    def get_tuple(self, config: RunnableConfig) -> CheckpointTuple | None: ...
    def put(self, config, checkpoint, metadata, new_versions) -> RunnableConfig: ...
    def put_writes(self, config, writes, task_id, task_path): ...
    def list(self, config, *, filter, before, limit) -> Iterator[CheckpointTuple]: ...
```

**关键设计**：
- `Checkpoint` 是 TypedDict，包含完整的状态快照
- `BaseCheckpointSaver[V]` 是泛型基类，支持不同的序列化方式
- `serde` 是可插拔的序列化协议（默认 `JsonPlusSerializer`）
- Checkpoint 与执行引擎解耦，通过 `PregelLoop.checkpointer` 注入

### 2.4 Streaming 系统

```python
# langgraph/pregel/protocol.py
class StreamProtocol(Protocol):
    modes: set[StreamMode]
    def __call__(self, value: StreamChunk) -> None: ...

# StreamMode 类型
StreamMode = Literal[
    "values",      # 每个 superstep 后的完整状态
    "updates",     # 每个 node 的增量更新
    "messages",    # LLM token 流
    "custom",      # 用户自定义事件
    "checkpoints", # checkpoint 事件
    "tasks",       # task 生命周期
    "debug",       # 调试信息
]

# 使用方式
class PregelLoop:
    def __init__(self, ..., stream: StreamProtocol | None, ...):
        self.stream = stream
    
    def _emit(self, mode: StreamMode, data: Any):
        if self.stream and mode in self.stream.modes:
            self.stream((mode, data))
```

**关键设计**：
- `StreamProtocol` 是统一的流接口
- 支持多种 stream mode，每种 mode 有不同的数据格式
- Stream 是可选的，通过 `StreamProtocol | None` 注入
- 支持 `DuplexStream` 组合多个 stream

### 2.5 执行算法

```python
# langgraph/pregel/_algo.py
def apply_writes(
    checkpoint: Checkpoint,
    channels: Mapping[str, BaseChannel],
    tasks: Iterable[WritesProtocol],
    get_next_version: GetNextVersion | None,
    trigger_to_nodes: Mapping[str, Sequence[str]],
) -> set[str]:
    """将 task writes 应用到 channels 和 checkpoint"""
    # 1. 更新 versions_seen
    for task in tasks:
        checkpoint["versions_seen"].setdefault(task.name, {}).update(...)
    
    # 2. 消费被读取的 channels
    for chan in {chan for task in tasks for chan in task.triggers}:
        if channels[chan].consume():
            checkpoint["channel_versions"][chan] = next_version
    
    # 3. 按 channel 分组 writes
    pending_writes_by_channel: dict[str, list[Any]] = defaultdict(list)
    for task in tasks:
        for chan, val in task.writes:
            pending_writes_by_channel[chan].append(val)
    
    # 4. 应用 writes 到 channels
    updated_channels: set[str] = set()
    for chan, vals in pending_writes_by_channel.items():
        if channels[chan].update(vals):
            checkpoint["channel_versions"][chan] = next_version
            updated_channels.add(chan)
    
    return updated_channels

def prepare_next_tasks(
    checkpoint: Checkpoint,
    processes: Mapping[str, PregelNode],
    channels: Mapping[str, BaseChannel],
    ...
) -> dict[str, PregelExecutableTask]:
    """决定下一批要执行的 tasks"""
    # 1. 检查哪些 channels 有更新
    # 2. 根据 trigger_to_nodes 映射找到要触发的 nodes
    # 3. 创建 PregelExecutableTask 实例
    # 4. 返回 task_id -> task 映射
```

**关键设计**：
- `apply_writes` 是核心算法，将 writes 应用到 channels 和 checkpoint
- `prepare_next_tasks` 决定下一批 tasks，基于 channel 更新和 trigger 映射
- `trigger_to_nodes` 是 channel -> nodes 的映射，用于决定哪些 node 被触发
- `versions_seen` 跟踪每个 node 见过的 channel 版本，避免重复执行

---

## 3. Loom 当前架构痛点（基于源码对比）

| # | 痛点 | 严重度 | LangGraph 做法 |
|---|------|--------|---------------|
| 1 | 双轨执行系统 (graph-core vs pregel) | 🔴 High | 单一 PregelLoop |
| 2 | JSON 类型擦除 (`serde_json::Value`) | 🔴 High | TypedDict + Annotated reducer |
| 3 | Streaming 层断裂，丰富事件未使用 | 🔴 High | StreamProtocol + 多 mode |
| 4 | 两套独立 Channel 实现 | 🟡 Medium | 统一 BaseChannel |
| 5 | 错误信息丢失 (String-only) | 🟡 Medium | GraphInterrupt + RetryPolicy |
| 6 | Checkpoint 与 Pregel 紧耦合 | 🟡 Medium | BaseCheckpointSaver 可插拔 |
| 7 | Node trait 不够灵活 | 🟡 Medium | PregelNode 继承 Runnable |
| 8 | 执行循环单体化 (130+ 行) | 🟢 Low | tick() + apply_writes() + prepare_next_tasks() 分离 |

---

## 4. 重构方案（基于 LangGraph 源码）

### 4.1 目标架构

```
┌─────────────────────────────────────────────────────────────┐
│                      User API Layer                          │
│  StateGraph<S> → compile() → CompiledGraph<S>               │
│  (泛型保留，Node<S> 强类型)                                   │
├─────────────────────────────────────────────────────────────┤
│                   Execution Engine                            │
│  PregelLoop<S> — BSP 并行执行                                │
│  ├─ tick(): 执行一个 superstep                               │
│  ├─ apply_writes(): 应用 writes 到 channels                 │
│  └─ prepare_next_tasks(): 决定下一批 tasks                   │
├─────────────────────────────────────────────────────────────┤
│                   State & Channels                            │
│  Channel<S> — 泛型 channel，保留类型                          │
│  ├─ LastValue<S>, Topic<S>, Aggregate<S>                    │
│  └─ StateSchema trait — 编译期 schema 验证                    │
├─────────────────────────────────────────────────────────────┤
│                   Streaming                                   │
│  StreamProtocol<S> — 统一事件流                               │
│  ├─ modes: values, updates, messages, tasks, debug          │
│  └─ StreamSink<S> trait — 自定义消费                          │
├─────────────────────────────────────────────────────────────┤
│                   Persistence                                 │
│  CheckpointSaver<S> trait — 可插拔                           │
│  ├─ SqliteSaver, MemorySaver                                │
│  └─ 与执行引擎解耦，通过 PregelLoop.checkpointer 注入         │
└─────────────────────────────────────────────────────────────┘
```

### 4.2 分阶段重构计划

#### Phase 1: 统一执行引擎 (消除 graph-core vs pregel 双轨)

**目标**：删除 `graph-core` 的独立执行路径，让 `StateGraph` 直接编译为 `PregelLoop`。

**步骤**：
1. 将 `graph-core::StateGraph` 迁移到 `pregel` crate（或新建 `graph` crate）
2. `StateGraph::compile()` 返回 `PregelLoop<S>` 而非 `CompiledStateGraph`
3. 统一 Node trait：
   ```rust
   #[async_trait]
   pub trait Node<S>: Send + Sync {
       async fn run(&self, state: &S, ctx: NodeContext) -> Result<NodeOutput<S>, GraphError>;
   }
   
   pub enum NodeOutput<S> {
       Update(PartialUpdate<S>),     // 增量更新
       Replace(S),                    // 全量替换
       Command { update: PartialUpdate<S>, goto: String },  // 带路由
   }
   ```
4. 删除 `graph-core::CompiledStateGraph` 和顺序执行路径

**涉及文件**：
- `foundation/graph-core/src/graph.rs` → 迁移
- `foundation/graph-core/src/node.rs` → 合并到 pregel
- `foundation/pregel/src/runtime.rs` → 接收 StateGraph 编译逻辑

**风险**：中等。需要确保所有现有 node 实现迁移到新 trait。

#### Phase 2: 泛型状态系统 (消除 JSON 类型擦除)

**目标**：让 `PregelLoop<S>` 泛型化，channel 值保留 Rust 类型。

**步骤**：
1. 定义 `StateSchema` trait：
   ```rust
   pub trait StateSchema: Clone + Send + Sync + 'static {
       type Channels: ChannelBundle;
       fn to_channels(&self) -> Self::Channels;
       fn from_channels(channels: Self::Channels) -> Self;
   }
   ```
2. 修改 `Checkpoint<S>` 使用泛型 `S` 而非 `serde_json::Value`
3. 修改 `PregelLoop<S>` 全程使用泛型 `S`
4. 提供 `serde_json::Value` 作为 fallback（用于动态场景）

**涉及文件**：
- `foundation/pregel/src/runtime.rs`
- `foundation/pregel/src/channel.rs`
- `foundation/checkpoint/src/checkpoint.rs`

**风险**：高。这是核心类型变更，影响面广。建议先做 Phase 1 稳定后再进行。

#### Phase 3: 统一 Streaming 层

**目标**：让 pregel 使用 `stream-event` 定义的完整事件类型，并添加 projection 层。

**步骤**：
1. 定义 `StreamProtocol` trait：
   ```rust
   pub trait StreamProtocol<S>: Send + Sync {
       fn modes(&self) -> &StreamModes;
       fn emit(&self, chunk: StreamChunk<S>);
   }
   
   pub enum StreamMode {
       Values,      // 每个 superstep 后的完整状态
       Updates,     // 每个 node 的增量更新
       Messages,    // LLM token 流
       Tasks,       // task 生命周期
       Debug,       // 调试信息
   }
   ```
2. 在 pregel 执行循环中 emit 完整事件
3. 实现 `StreamLayer<S>` 提供 projection API：
   ```rust
   impl<S> PregelStream<S> {
       pub fn values(&self) -> impl Stream<Item = S>;
       pub fn updates(&self) -> impl Stream<Item = NodeUpdate<S>>;
       pub fn messages(&self) -> impl Stream<Item = MessageChunk>;
       pub fn tasks(&self) -> impl Stream<Item = TaskEvent>;
   }
   ```

**涉及文件**：
- `stream-event/src/stream_event.rs`
- `foundation/pregel/src/runtime.rs`
- 新建 `foundation/pregel/src/stream.rs`

**风险**：低。这是纯增量变更，不破坏现有 API。

#### Phase 4: Checkpoint 解耦

**目标**：将 checkpoint 持久化从 Pregel 执行循环中解耦。

**步骤**：
1. 定义 `CheckpointSaver<S>` trait：
   ```rust
   #[async_trait]
   pub trait CheckpointSaver<S>: Send + Sync {
       async fn get_tuple(&self, config: &CheckpointConfig) -> Result<Option<CheckpointTuple<S>>, CheckpointError>;
       async fn put(&self, config: &CheckpointConfig, checkpoint: &Checkpoint<S>, metadata: &CheckpointMetadata) -> Result<CheckpointConfig, CheckpointError>;
       async fn put_writes(&self, config: &CheckpointConfig, writes: &[(String, S)], task_id: &str) -> Result<(), CheckpointError>;
   }
   ```
2. 修改 `PregelLoop` 接受 `Option<Arc<dyn CheckpointSaver<S>>>`
3. 支持多个 saver 实现（SQLite, Memory, 等）

**涉及文件**：
- `foundation/pregel/src/runtime.rs`
- `foundation/checkpoint/src/checkpointer.rs`
- 新建 `foundation/checkpoint/src/saver.rs`

**风险**：中等。需要确保 checkpoint 时序正确。

#### Phase 5: 错误处理增强

**目标**：结构化错误，保留错误上下文。

**步骤**：
1. 重新设计 `GraphError`：
   ```rust
   pub enum GraphError {
       NodeFailed { node_id: String, source: Box<dyn Error> },
       ChannelError { channel: String, source: Box<dyn Error> },
       CheckpointError { source: Box<CheckpointError> },
       Interrupted { record: InterruptRecord },
       Cancelled,
   }
   ```
2. 添加 `RetryPolicy` 支持：
   ```rust
   pub struct RetryPolicy {
       max_attempts: u32,
       backoff: BackoffStrategy,
       retryable_errors: Vec<ErrorPredicate>,
   }
   ```
3. 支持 node 级别的错误恢复 hook

**涉及文件**：
- `foundation/graph-core/src/error.rs`
- `foundation/pregel/src/runtime.rs`

**风险**：低。向后兼容，只是增加信息。

---

## 5. 优先级建议

| 阶段 | 优先级 | 理由 |
|------|--------|------|
| Phase 3: Streaming 统一 | 🔴 P0 | 增量变更，立即解锁 TUI 需要的丰富事件 |
| Phase 1: 统一执行引擎 | 🔴 P0 | 消除双轨，降低维护成本 |
| Phase 5: 错误处理 | 🟡 P1 | 改善调试体验，风险低 |
| Phase 4: Checkpoint 解耦 | 🟡 P1 | 为未来扩展（如多持久化后端）铺路 |
| Phase 2: 泛型状态 | 🟢 P2 | 收益大但风险高，建议其他阶段完成后进行 |

---

## 6. 与 LangGraph 的关键差异（保留）

Loom 不需要完全复制 LangGraph 的设计。以下是建议保留的差异：

| 方面 | LangGraph | Loom (建议) | 理由 |
|------|-----------|-------------|------|
| 语言 | Python (动态) | Rust (静态) | 利用 Rust 类型系统 |
| 状态传递 | 字典 + reducer | 泛型 struct + Channel trait | 类型安全 |
| Node 签名 | `State -> Partial<State>` | `&S -> NodeOutput<S>` | 避免 clone 开销 |
| 子图 | 自动 namespace | 显式 namespace + 类型约束 | Rust 需要明确边界 |
| Checkpoint | 可插拔 saver | 可插拔 saver | 一致 |
| Streaming | StreamProtocol | StreamProtocol | 一致 |

---

## 7. 迁移路径

对于现有代码，建议渐进式迁移：

1. **短期 (1-2 周)**：Phase 3 — 在 pregel 中 emit 完整 StreamEvent
2. **中期 (2-4 周)**：Phase 1 — 统一 StateGraph 和 PregelRuntime
3. **长期 (1-2 月)**：Phase 2 + 4 — 泛型状态 + checkpoint 解耦

每个阶段都应该是可独立发布的，不要求一次性完成。

---

## 8. 源码参考

- `langgraph/pregel/_loop.py` — PregelLoop 执行循环
- `langgraph/pregel/_algo.py` — apply_writes, prepare_next_tasks
- `langgraph/channels/base.py` — BaseChannel 抽象
- `langgraph/channels/last_value.py` — LastValue 实现
- `langgraph/channels/topic.py` — Topic 实现
- `langgraph/checkpoint/base/__init__.py` — Checkpoint, BaseCheckpointSaver
- `langgraph/pregel/protocol.py` — StreamProtocol
