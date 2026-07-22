# LangGraph Checkpoint 对齐开发计划

## 1. 目标与结论

本计划的目标是把 Loom 的 checkpoint 语义对齐到 LangGraph 风格：以 bulk-synchronous super-step 为持久化边界，支持每步 checkpoint、pending writes 容错、`sync`/`async`/`exit` durability、interrupt/resume、history/replay/update state/fork，并让主 agent runner 真正使用这套语义。

当前关键结论：

- `foundation/graph-core` 的 `CompiledStateGraph` 是现有主 agent 的执行内核。正常路径只在图结束时保存最终 checkpoint；节点中断时额外保存一次中断前状态。
- `agent-core` 的 ReAct/DUP/ToT/GoT runner 仍基于 `CompiledStateGraph`，因此主 agent 当前实际是 final-only checkpoint。
- `foundation/pregel` 已经实现更接近 LangGraph 的 runtime：super-step/barrier 循环、`PregelDurability::{Sync, Async, Exit}`、`pending_sends`/`pending_writes`/`pending_interrupts`、`get_state`、`get_state_history`、`update_state`、`replay`、`fork`。
- `foundation/checkpoint-sqlite-store` 的 `checkpoints` 表已能存完整 checkpoint 和 `pending_*` JSON 字段，但缺少 LangGraph 风格的独立 checkpoint writes 表/API。
- 目前最大的缺口不是底层完全缺失，而是 Pregel 没有成为 StateGraph/主 agent 的默认执行路径，并且外部 API、snapshot shape、SQLite schema、CLI/ACP 能力还没有完整对齐。

## 2. 当前实现分析

### 2.1 `foundation/checkpoint`

现有 checkpoint 类型已经具备对齐基础：

- `Checkpoint<S>` 存储 `id`、`ts`、`channel_values`、`channel_versions`、`versions_seen`、`updated_channels`、`pending_sends`、`pending_writes`、`pending_interrupts`、`kernel`。
- `KernelMetadata` 存储 `source`、`step`、`created_at`、`parents`、`children`、`summary`。
- `RunnableConfig` 已包含 `thread_id`、`checkpoint_id`、`checkpoint_ns`、`user_id`、`resume_*`、`resume_from_node_id`。
- `Checkpointer<S>` 当前只有 `put`、`get_tuple`、`list`。

缺口：

- 没有 `put_writes`/`aput_writes` 级别接口。
- `CheckpointTuple` 有 `pending_writes` 和 `parent_config` 字段，但 trait 返回值仍是 `(Checkpoint<S>, CheckpointMetadata)`，没有完整暴露 tuple。
- `checkpoint_ns` 是单个 `String`，对嵌套子图路径表达不如 LangGraph 的 namespace tuple/list。

### 2.2 `foundation/checkpoint-sqlite-store`

现有 SQLite saver：

- 主表 `checkpoints(thread_id, checkpoint_ns, checkpoint_id, ts, payload, channel_versions, versions_seen, metadata_*, updated_channels, pending_sends, pending_writes, pending_interrupts)`。
- `put` 把完整 state snapshot 和 pending fields 一起写入 `checkpoints`。
- `get_tuple` 支持按 `checkpoint_id` 读取指定 checkpoint，或读取 thread/ns 下最新 checkpoint。
- `list` 支持按 lineage 列出 metadata。

缺口：

- 没有独立 `checkpoint_writes` 表，无法在一个 super-step 内对每个 task 的成功写入做细粒度持久化。
- `list` 当前只返回 metadata，不返回 config、parent_config、pending writes。
- `metadata_created_at` 是排序核心，需要确认所有新路径都稳定设置。
- 缺少 schema migration/version gate，用于平滑增加 writes 表和索引。

### 2.3 `foundation/graph-core`

`CompiledStateGraph` 当前 checkpoint 行为：

- 正常执行：节点循环完成、路由到 `END` 或 `next_id == None` 时创建 `Checkpoint::from_state(state, Update, 0)` 并 `cp.put(...)`。
- 中断执行：捕获 `GraphError::Interrupted` 时保存当前 `state`，再返回中断错误。
- 不保存输入 checkpoint，不保存每个节点后的 checkpoint，不保存 task-level writes。
- `stream_mode=Checkpoints` 只在上述 checkpoint 创建后发事件。

影响：

- 对简单图来说，history 只有最终状态，不能 time travel 到中间节点。
- 对主 agent 来说，工具调用前后的中间状态无法恢复；进程崩溃时最多回到上一轮最终状态。
- `metadata_step` 目前多处为 `0`，不足以表达每个 super-step 的历史。

### 2.4 `foundation/pregel`

Pregel runtime 已经具备主要对齐基础：

- `PregelRuntime::invoke_inner` 按 `tick -> run_step -> after_tick -> persist` 循环执行。
- `PregelDurability::Sync`：每步后同步 `persist_checkpoint`，默认。
- `PregelDurability::Async`：后台写 checkpoint，下一步继续跑，退出/错误前 flush。
- `PregelDurability::Exit`：运行退出时才持久化。
- `PregelLoop::after_tick` 合并 successful writes、interrupts、errors、cancel，并推进 `checkpoint.kernel.step`。
- `PregelStateSnapshot` 暴露 checkpoint id、step、channels、parents、children、updated channels、pending sends/writes/interrupts。
- `get_state`、`get_state_history`、`update_state`、`bulk_update_state`、`replay` 已存在。

缺口：

- 初始 `CheckpointSource::Input` 是内存 checkpoint；无历史时不立即落库，因此没有 LangGraph 那种 input checkpoint 历史。
- pending writes 仍是完整 checkpoint 的字段，不是独立 task-write journal。
- `StateSnapshot` 与 LangGraph 的 `values/config/metadata/next/tasks/parent_config` 形状不一致。
- `checkpoint_ns` 仍是字符串，子图 namespace lineage 可用但不是路径类型。
- Pregel Graph 是低层 API，主 agent runner 还没有迁移。

### 2.5 `agent-core`

主 agent runner 当前路径：

- ReAct/DUP/ToT/GoT 构建 `StateGraph<S>`。
- 编译为 `CompiledStateGraph<S>` 或 `compile_with_checkpointer(...)`。
- `runner_common::run_stream_with_config` 调用 `compiled.stream(...)`。
- 初始状态通过 `load_from_checkpoint_or_build` 读取最新 checkpoint 后合并用户消息。

影响：

- 即使底层 Pregel 已支持每步 checkpoint，用户跑主 agent 时仍不使用 Pregel。
- 当前 session/checkpoint 行为主要是 conversation continuity，而不是 LangGraph 风格 runtime recovery/time travel。

## 3. LangGraph 目标语义

### 3.1 checkpoint 边界

目标行为：

- graph run 由一系列 super-step 组成。
- 每个 super-step 完成后产生一个 checkpoint。
- checkpoint 包含该 barrier 后的 channel values、channel versions、versions seen、updated channels、pending sends/writes/interrupts、metadata。
- 初始输入也应形成可追踪 checkpoint，使 history 能表达 run 从 input 到 final 的全过程。

### 3.2 pending writes

LangGraph 的关键容错点是：同一 super-step 中某些 task 成功、某些 task 失败时，成功 task 的 writes 可以先持久化。恢复时这些成功 task 不应重跑。

Loom 目标：

- 在 `Checkpointer` 增加 `put_writes(config, checkpoint_id, task_id, writes)`。
- SQLite 新增 `checkpoint_writes` 表。
- `PregelRunner` 或 `PregelLoop::after_tick` 在 task 成功后、barrier 完整 checkpoint 前写入 task-level writes。
- 下一次恢复时先加载 checkpoint，再附加 checkpoint writes，构造 cached writes/pending writes。

### 3.3 durability

目标语义：

- `sync`：每个 checkpoint 在下一步开始前必须持久化成功。最安全，默认。
- `async`：checkpoint 后台写，下一步可以继续；退出、错误、中断、取消前必须 flush。
- `exit`：运行中不写每步 checkpoint，只在退出时写最终可恢复 checkpoint。性能优先，容错较弱。

Loom 已有 `PregelDurability`，需要把它暴露到高层配置和 CLI/ACP。

### 3.4 interrupt/resume

目标语义：

- interrupt 前必须保存足够恢复的信息。
- checkpoint 需要包含 pending interrupts 和 resume routing 信息。
- resume 可以按 namespace 或 interrupt id 注入 resume value。
- resume 后应消费对应 interrupt，保留未消费 interrupts。

Loom 已有 `resume_value`、`resume_values_by_namespace`、`resume_values_by_interrupt_id` 和 `pending_interrupts`，需要补齐高层入口、测试和 snapshot 表达。

### 3.5 history/replay/update/fork

目标能力：

- `get_state(config)`：读取当前/latest snapshot。
- `get_state_history(config)`：按 checkpoint lineage 列历史。
- `update_state(config, values, as_node)`：通过 write barrier 写入外部状态更新，并产生新 checkpoint。
- `replay(checkpoint_id)`：从历史 checkpoint 重放。
- `fork(checkpoint_id, new namespace/thread)`：从历史 checkpoint 分叉 lineage，不污染原 lineage。

Pregel 已有基础，需要向主 agent/API/CLI/ACP 暴露。

## 4. 差距清单

| 领域 | 当前状态 | 目标状态 | 优先级 |
| --- | --- | --- | --- |
| 主 agent runtime | ReAct/DUP/ToT/GoT 使用 `CompiledStateGraph` | 使用 Pregel-backed runtime 或 StateGraph 编译到 Pregel | P0 |
| checkpoint 边界 | graph-core final-only，中断保存一次 | 每个 super-step/barrier checkpoint | P0 |
| input checkpoint | Pregel 创建但不落库 | history 中可见 input checkpoint | P1 |
| pending writes | 存在 checkpoint 字段 | 独立 writes journal + `put_writes` API | P1 |
| durability 配置 | Pregel 低层有 | 高层 agent/CLI/ACP 可配置 | P1 |
| snapshot shape | `PregelStateSnapshot` 内部字段 | LangGraph-like `StateSnapshot` | P2 |
| namespace | `String checkpoint_ns` | 路径 namespace | P2 |
| CLI/ACP | 主要 session latest checkpoint | get/history/replay/fork/update/resume | P2 |
| tests | 单元较多，E2E不足 | checkpoint recovery E2E | P0/P1 |

## 5. 架构方案

### 5.1 推荐方向：StateGraph 编译到 Pregel

推荐把 `StateGraph<S>` 保留为高层用户 API，但新增 Pregel-backed compiled runtime：

- `StateGraph::compile_pregel(...) -> CompiledPregelStateGraph<S>`
- `StateGraph::compile_with_checkpointer(...)` 可在兼容期继续返回旧 `CompiledStateGraph`。
- 新增 opt-in 配置：`checkpoint_runtime = "pregel"` 或 `StateGraphCompileMode::Pregel`。
- 主 agent runner 先显式使用 Pregel-backed path，验证稳定后再考虑切默认。

优点：

- 对用户保留 StateGraph API。
- 复用 Pregel checkpoint/replay/history 能力。
- 迁移主 agent 时不用立刻重写全部 agent node 业务逻辑。

难点：

- `StateGraph<S>` 当前是 state-in/state-out 的 whole-state model；Pregel 是 channel model。需要一个 adapter，把整个 `S` 作为 single channel，或把 state fields 映射到 channels。
- `Node<S>` 返回 `(S, Next)`，Pregel node 返回 writes。需要 adapter 把 node output 写回 state channel，并把 `Next` 写到 routing/control channel。
- conditional edges、middleware、retry、interrupt、stream events、metadata extractor 都要保留语义。

### 5.2 备选方向：主 agent runner 直接迁移到 PregelGraph

可直接为 ReAct/DUP/ToT/GoT 各自构建 PregelGraph：

- channels：`state`、`next`、`messages`、`tool_calls` 等。
- nodes：`think`、`act`、`observe`、`compress`。
- triggers：通过 control channel 驱动循环。

优点：

- 最贴近 Pregel 模型，checkpoint 语义清晰。
- 可针对 agent loop 做更细粒度 channels。

缺点：

- 改动面大，ReAct/DUP/ToT/GoT 各自迁移成本高。
- 需要重复解决 StateGraph 已经有的 conditional edges/middleware 语义。

### 5.3 推荐落地策略

先做 StateGraph-to-Pregel adapter 的最小版本，只支持主 agent 当前使用的能力：

- single state channel。
- linear/conditional routing。
- middleware/retry pass-through。
- checkpoint metadata extractor。
- stream `Values/Updates/Checkpoints/Tasks`。

待主 agent 稳定后，再扩展到更完整的 public StateGraph Pregel compile。

## 6. 分阶段实施计划

### Phase 0：基线测试与行为锁定

目标：在改动前用测试锁住当前行为和目标差距。

涉及文件：

- `foundation/graph-core/src/compiled.rs`
- `foundation/pregel/src/runtime.rs`
- `foundation/pregel/src/loop_state.rs`
- `foundation/checkpoint-sqlite-store/src/sqlite_saver.rs`
- 新增 `foundation/pregel/tests/checkpoint_semantics.rs`
- 新增 `agent/agent-core/tests/checkpoint_runner_semantics.rs`

任务：

1. 为 `CompiledStateGraph` 增加测试：正常路径只产生最终 checkpoint，中断产生 interrupt checkpoint。
2. 为 `PregelRuntime` 增加测试：`Sync` 每 step checkpoint、`Async` flush、`Exit` 只退出 checkpoint。
3. 为主 ReAct runner 增加测试：当前只保存最终 checkpoint，作为迁移前 baseline。
4. 建立 test helpers：in-memory checkpointer 记录 checkpoint count、step、source、updated channels。

验收标准：

- 测试能明确证明当前差距。
- 不改变生产代码行为。
- `cargo test -p loom-pregel checkpoint` 和 `cargo test -p loom-graph-core checkpoint` 通过。

### Phase 1：补齐 checkpoint writer API 与 SQLite writes journal

目标：支持 LangGraph 风格 task-level pending writes。

涉及文件：

- `foundation/checkpoint/src/checkpointer.rs`
- `foundation/checkpoint/src/checkpoint.rs`
- `foundation/checkpoint/src/memory_saver.rs`
- `foundation/checkpoint-sqlite-store/src/sqlite_saver.rs`
- `foundation/checkpoint-sqlite-store/src/repair.rs`
- `foundation/pregel/src/runtime.rs`
- `foundation/pregel/src/runner.rs`
- `foundation/pregel/src/loop_state.rs`

任务：

1. 在 `Checkpointer<S>` 增加：
   - `put_writes(config, checkpoint_id, task_id, writes)`
   - `get_writes(config, checkpoint_id)` 或扩展 `get_tuple` 返回 full tuple。
2. SQLite 新增表：

```sql
CREATE TABLE IF NOT EXISTS checkpoint_writes (
    thread_id TEXT NOT NULL,
    checkpoint_ns TEXT NOT NULL,
    checkpoint_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    idx INTEGER NOT NULL,
    channel TEXT NOT NULL,
    value BLOB NOT NULL,
    created_at INTEGER,
    PRIMARY KEY (thread_id, checkpoint_ns, checkpoint_id, task_id, idx)
);
CREATE INDEX IF NOT EXISTS idx_checkpoint_writes_lineage
ON checkpoint_writes(thread_id, checkpoint_ns, checkpoint_id);
```

3. `MemorySaver` 实现同等 writes 存储，方便测试。
4. Pregel task 成功后先 `put_writes`，barrier checkpoint 成功后可保留或清理 writes，具体策略：
   - 保留：便于审计和 replay，成本是存储增长。
   - 清理：节省空间，但降低 debug 能力。
   推荐先保留，并提供 repair/prune 工具。
5. 恢复时加载 checkpoint + writes，合并到 pending/cached writes。

验收标准：

- 同一 super-step 中一个 task 成功、一个 task 失败，成功 task writes 已持久化。
- 重试/恢复时成功 task 不重跑。
- SQLite 和 MemorySaver 行为一致。

### Phase 2：补齐 Pregel checkpoint 语义

目标：让 Pregel 本身严格对齐目标语义。

涉及文件：

- `foundation/pregel/src/runtime.rs`
- `foundation/pregel/src/loop_state.rs`
- `foundation/pregel/src/state.rs`
- `foundation/pregel/src/replay.rs`
- `foundation/pregel/src/subgraph.rs`
- `foundation/pregel/src/config.rs`

任务：

1. 持久化 input checkpoint：
   - 无历史 checkpoint 时，在 `init_loop` 或 `invoke_inner` 开始处写入 `CheckpointSource::Input`。
   - 避免重复写：同一 run resume 时不重复创建 input checkpoint。
2. 统一 step 编号：
   - input checkpoint 使用 `step = -1` 或 `0` 需明确决策。
   - loop checkpoint 递增，history 顺序稳定。
3. 扩展 `PregelStateSnapshot`：
   - `values`
   - `next`
   - `tasks`
   - `config`
   - `metadata`
   - `parent_config`
   - 保留 `pending_*` 作为 Loom 扩展字段。
4. interrupt/resume：
   - 明确 interrupt-before 与 interrupt-after 的 checkpoint 保存点。
   - resume 后消费对应 interrupt，未消费 interrupt 留在 snapshot。
5. replay/fork：
   - `ResumeFromCheckpoint` 不应覆盖历史 checkpoint。
   - `ForkFromCheckpoint` 应建立 parent/children lineage。

验收标准：

- 一个两节点 Pregel 图 history 至少包含 input、node A 后、node B 后/final。
- `get_state_history` 按 step/created_at 稳定排序。
- interrupt 后 `get_state` 能看到 pending interrupt，resume 后消失或只保留未消费项。
- fork 后原 lineage 和新 lineage 都可查询。

### Phase 3：StateGraph-to-Pregel adapter

目标：让高层 StateGraph 能使用 Pregel checkpoint 语义。

涉及文件：

- `foundation/graph-core/src/state_graph.rs`
- `foundation/graph-core/src/compiled.rs`
- 新增 `foundation/graph-core/src/pregel_adapter.rs`
- `foundation/graph-core/src/run_context.rs`
- `foundation/graph-core/src/node.rs`
- `foundation/pregel/src/node.rs`
- `foundation/pregel/src/channel.rs`

任务：

1. 新增 `CompiledPregelStateGraph<S>`：
   - 持有 `PregelRuntime`。
   - 对外提供 `invoke`、`stream`、`get_state`、`get_state_history`。
2. 定义 single-state channel adapter：
   - `__state__` 保存完整 `S`。
   - `__next__` 保存 routing target。
   - `__end__` 或 reserved write 表达终止。
3. 把 `Node<S>` 包装为 `PregelNode`：
   - 从 `__state__` 读取 state。
   - 调用原 node `run_with_context`。
   - 写回 `__state__`。
   - 将 `Next`/conditional routing 写入 control channel。
4. 适配 conditional edges：
   - 先执行 node，再用 router resolve target。
   - 与现有 `CompiledStateGraph` 路由行为保持一致。
5. 适配 middleware/retry/interrupt/cancellation。
6. 适配 stream events：
   - `Values` 仍发完整 `S`。
   - `Updates` 可先发完整 state + node id，后续再做 diff。
   - `Checkpoint` 来源于 Pregel checkpoint event。
7. metadata extractor：
   - checkpoint 写入前把 summary 写进 metadata。

验收标准：

- 现有 StateGraph 单元测试在 Pregel-backed 模式下通过。
- 简单 ReAct-like 循环在每个 node/barrier 后产生 checkpoint。
- 旧 `compile`/`compile_with_checkpointer` 行为不破坏，新增 API opt-in。

### Phase 4：主 agent runner 迁移

目标：让 ReAct/DUP/ToT/GoT 主路径使用 Pregel-backed checkpoint。

涉及文件：

- `agent/agent-core/src/agent/react/runner/runner.rs`
- `agent/agent-core/src/agent/dup/runner.rs`
- `agent/agent-core/src/agent/tot/runner.rs`
- `agent/agent-core/src/agent/got/runner.rs`
- `agent/agent-core/src/runner_common.rs`
- `agent/agent-core/src/agent/react/build/runners.rs`
- `agent/agent-core/src/agent/react/build/checkpointer.rs`
- `agent/agent-core/src/run/types.rs`
- `agent/agent-core/src/run/config_builder.rs`

任务：

1. 增加 runtime 选择配置：
   - `checkpoint_runtime = "compiled" | "pregel"`
   - 默认先保持 `"compiled"`，测试和实验 profile 使用 `"pregel"`。
2. `ReactRunner` 支持两种 compiled backend：
   - `CompiledStateGraph<ReActState>`
   - `CompiledPregelStateGraph<ReActState>`
3. `runner_common::run_stream_with_config` 泛化到 trait：
   - `GraphRunner<S>::stream(...)`
   - 避免 runner_common 直接依赖 `CompiledStateGraph`。
4. `load_from_checkpoint_or_build` 对 Pregel snapshot 适配：
   - latest checkpoint values -> state。
   - 合并 user message 的位置保持不变。
5. 逐个迁移 DUP/ToT/GoT。

验收标准：

- ReAct 在 Pregel mode 下每轮 think/act/observe/compress 都产生 checkpoint。
- 工具执行后进程崩溃可从最近 checkpoint/pending writes 恢复。
- legacy compiled mode 仍可运行。
- 主 CLI 默认行为无破坏；实验 flag 可启用 Pregel checkpoint。

### Phase 5：CLI/ACP/API 暴露

目标：让用户可以查询和操作 checkpoint history。

涉及文件：

- `apps/cli/src/args.rs`
- `apps/cli/src/session.rs`
- `apps/cli/src/subcommands.rs`
- 新增 `apps/cli/src/checkpoint_cmd.rs`
- `apps/acp/src/agent.rs`
- `apps/acp/src/session.rs`
- `apps/acp/src/protocol.rs`
- `apps/acp/src/client_methods.rs`
- `agent/agent-core/src/tools/thread_get.rs`

CLI 设计建议：

```text
loom checkpoint state --session-id <id>
loom checkpoint history --session-id <id> [--limit N]
loom checkpoint inspect --session-id <id> --checkpoint-id <cid>
loom checkpoint replay --session-id <id> --checkpoint-id <cid>
loom checkpoint fork --session-id <id> --checkpoint-id <cid> [--namespace <ns>]
loom checkpoint update --session-id <id> --json <values> [--as-node <node>]
```

ACP 能力建议：

- `session/state`
- `session/stateHistory`
- `session/replay`
- `session/fork`
- `session/updateState`
- resume interrupt 的 request/response 字段。

验收标准：

- CLI 能列出一个 session 的 checkpoint history。
- ACP session load 能返回 latest state + history metadata。
- replay/fork/update 操作不会破坏原 session。

### Phase 6：默认切换与清理

目标：在足够测试后，把主 agent 默认 checkpoint runtime 切到 Pregel。

任务：

1. 默认启用 Pregel-backed runtime。
2. 保留 legacy fallback 一个版本周期。
3. 文档更新：
   - README checkpoint 说明。
   - migration guide。
   - CLI help。
4. 指标与日志：
   - checkpoint count。
   - checkpoint write latency。
   - pending writes recovery count。
   - async flush failures。
5. 清理 final-only 误导性注释或改为 legacy 说明。

验收标准：

- 默认 `loom` agent run 具备 per-step checkpoint。
- 旧 session 仍可读取。
- 性能回归可接受，`exit` durability 可作为性能逃生阀。

## 7. 测试计划

### 7.1 单元测试

- `checkpoint`：
  - `put_writes` trait default/impl 测试。
  - `CheckpointTuple` roundtrip。
  - namespace/path serialization。
- `checkpoint-sqlite-store`：
  - schema migration。
  - `checkpoint_writes` insert/list/dedupe。
  - latest checkpoint + writes restore。
- `pregel`：
  - durability 三模式。
  - input checkpoint。
  - interrupt/resume。
  - failed step pending writes recovery。
  - replay/fork/update state。
- `graph-core`：
  - StateGraph-to-Pregel adapter 路由。
  - conditional edges。
  - middleware/retry。
  - stream events。

### 7.2 集成测试

- ReAct runner：
  - think-only run。
  - think -> act -> observe -> compress -> think loop。
  - tool call 成功后 checkpoint。
  - tool call 后模拟失败，恢复不重复已成功 task。
- CLI：
  - `checkpoint history/state/inspect/update/fork`。
- ACP：
  - session load 返回 checkpoint-backed state。
  - session resume with pending interrupt。

### 7.3 兼容测试

- 旧 `checkpoints` 表无 `checkpoint_writes` 时自动 migrate。
- 旧 final-only session 可继续 `get_tuple`。
- legacy compiled mode 可通过 feature/config 保留。

## 8. 风险与兼容策略

### 8.1 行为变化风险

风险：默认 checkpoint 数量显著增加，session 列表、删除、搜索、统计都可能受影响。

策略：

- 先 opt-in，再默认切换。
- session UI 使用 latest checkpoint，不假设 checkpoint count 低。
- 增加 prune/compact 机制。

### 8.2 存储增长风险

风险：每步 checkpoint + writes journal 增加 SQLite 体积。

策略：

- 支持 checkpoint retention policy。
- 提供 `loom session prune` 或 `loom checkpoint prune`。
- 对大 payload 考虑 blob compression 或 external blob table。

### 8.3 性能风险

风险：`sync` 每步落库增加 latency。

策略：

- 默认先在实验期开 `sync`，测量后决定。
- 暴露 `async` 和 `exit`。
- 对 SQLite 开启 WAL、批量事务、必要索引。

### 8.4 API 兼容风险

风险：`Checkpointer` trait 增加方法会影响所有实现。

策略：

- 第一阶段给 `put_writes` 提供默认 no-op 或 default error。
- 同步升级 `MemorySaver` 和 `SqliteSaver`。
- 在版本文档中标记 trait breaking change。

### 8.5 StateGraph adapter 语义风险

风险：StateGraph whole-state model 到 Pregel channel model 映射不精确。

策略：

- 先 single-state channel，不做字段级 channels。
- 所有旧 StateGraph 测试在 adapter 下跑一遍。
- 迁移主 agent 前保留 legacy mode。

## 9. 推荐里程碑

1. 第 1 周：Phase 0 + Phase 1，完成 baseline 测试和 writes journal。
2. 第 2 周：Phase 2，补齐 Pregel input checkpoint、snapshot、interrupt/resume 测试。
3. 第 3-4 周：Phase 3，完成 StateGraph-to-Pregel adapter MVP。
4. 第 5 周：Phase 4，ReAct runner opt-in 迁移。
5. 第 6 周：DUP/ToT/GoT 迁移 + CLI checkpoint commands。
6. 第 7 周：ACP 集成 + E2E recovery 测试。
7. 第 8 周：默认切换评估、文档、性能和存储调优。

## 10. 最小可交付切片

如果需要尽快交付一个可验证版本，建议最小切片为：

1. `Checkpointer::put_writes` + SQLite `checkpoint_writes`。
2. Pregel input checkpoint + failed step pending writes recovery 测试。
3. ReAct runner opt-in Pregel-backed runtime。
4. CLI `checkpoint history` 和 `checkpoint inspect`。

这个切片能证明核心价值：主 agent 不再只保存最终状态，失败后能从最近 super-step/pending writes 恢复，并且用户能看到 checkpoint 历史。

