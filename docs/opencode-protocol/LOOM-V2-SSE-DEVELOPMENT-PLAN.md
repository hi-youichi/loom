# Loom v2 Session SSE 详细开发方案

状态：待实施。更新日期：2026-07-24。目标 OpenCode revision：`b8142c7aa`。

本方案是可直接排期和实现的 v2 SSE 设计，不是现状描述。现状、完整类型清单和差距分别见 [当前状态审计](CURRENT-STATE-AUDIT.md)、[逐类型修正矩阵](LOOM-V2-SSE-TYPE-MATRIX.md) 与 [修正方案](LOOM-V2-SSE-REMEDIATION.md)。本方案以 OpenCode 的 `packages/schema/src/event.ts`、`session-event.ts` 为唯一 schema 基准。

## 1. 交付范围、兼容边界与定义完成

最终目标是实现全部 32 个 `session.next.*` 类型；不支持的产品能力不得伪造 event。第一可用切片覆盖 **9 个 durable 类型**：

| 类型 | 数量 | 首个来源 |
| --- | ---: | --- |
| `prompted`、`prompt.admitted` | 2 | `prompt_v2()` admission 流程 |
| `step.started`、`step.ended`、`step.failed` | 3 | `TurnStart`、`TurnFinish`、`ProviderError` |
| `text.started`、`text.ended` | 2 | `TextBlockStart`、`TextBlockEnd` |
| `reasoning.started`、`reasoning.ended` | 2 | `ReasoningBlockStart`、`ReasoningBlockEnd` |

随后增加 4 个 live-only delta（text/reasoning/tool-input/compaction）、7 个 tool 事件和其余 12 个能力事件。完整逐类型责任、字段、版本和来源以矩阵为准；本文件为每阶段给出固定实现决策。

完成的定义：

1. `/api/session/:sessionID/event?after=<seq>` 只输出该 session 的 durable v2 event，按严格递增 sequence 回放并继续 live；它绝不输出 legacy part、`server.connected`、业务 heartbeat 或 live delta。
2. `/api/event` 输出 v2 全局 event（durable 和 live）；`/global/event` 和裸 `/event` 继续输出现有 legacy 流，TUI 不回归。
3. 每个已声明支持的 event 都有：OpenCode-shaped fixture、schema decode、publisher/translator 单测、replay 测试和 legacy 双发回归。
4. durable event 在成功落盘后才 broadcast，重启后仍可用相同 aggregate sequence 回放。
5. 未实现的 23 个类型在 capability 表中明确标为“不发布”，而不是发字段不完整的兼容假象。

## 2. 固定的 wire 与数据约定

### 2.1 外层 Envelope

每个 v2 event 使用下列 JSON；SSE frame 必须是 `event: message`，payload 放在 `data:`，不以 SSE `id:` 取代 JSON 内的 `id`。

```json
{
  "id": "evt_v2_01J...",
  "type": "session.next.text.ended",
  "data": {
    "timestamp": 1784876400000,
    "sessionID": "sess_...",
    "assistantMessageID": "msg_...",
    "textID": "part_...",
    "text": "完整文本"
  },
  "durable": { "aggregateID": "sess_...", "seq": 17, "version": 1 }
}
```

- `timestamp` 是 UTC Unix milliseconds，整数且非负。
- `id` 由 `new_v2_event_id()` 独立生成（推荐 `evt_v2_` + ULID）；不得重用 legacy `NEXT_EVENT_ID`。
- durable 的 `aggregateID` 必须等于 `data.sessionID`；`seq` 是该 session 从 1 开始的 `u64`，cursor 是 **exclusive**：`after=17` 返回 `seq > 17`。
- version：`step.ended`、`step.failed` 固定 2；其他 durable session event 固定 1；4 个 delta 没有 `durable`。
- `location`、`metadata` 只在 source 能提供且符合 OpenCode schema 时出现；首期一律省略，禁止传 legacy `{directory,payload}` 包装。

### 2.2 不能含糊的字段转换

| 目标字段 | 固定来源/规则 |
| --- | --- |
| `Model.Ref` | `{id: ModelInfo.model_id, providerID: ModelInfo.provider_id}`；存在 variant 才附 `variant`。没有有效 model 时 v2 prompt 以 409 拒绝，不能发空字段的 step。 |
| `finish` | `TurnFinish.reason` 经封闭映射为 OpenCode finish string；未知值映射 `"unknown"` 并记录 warning。映射表必须单测穷尽现有 enum。 |
| `cost` | 使用 session/message 已记录的 provider cost；当前 provider 没有计费值时明确发送 `0.0`（`Finite` 合法），不得阻塞 event 或伪造非零值。 |
| `tokens` | `{input,output,reasoning,cache:{read,write}}`；缺失项都为 `0`。只能使用有限非负数。 |
| `UnknownError` | 所有无专属 schema 的 Loom 错误为 `{type:"unknown",message:<已脱敏错误文本>}`。 |
| `ProviderMetadata` | 首期省略；后续只映射明确兼容的 string record，不透传任意 provider JSON。 |
| `Tool input` | 若 arguments 是 object 原样使用；否则用 `{value:<arguments>}`。`input.ended.text` 是同一输入的紧凑 JSON。 |
| `Tool content` | 首期 stdout/stderr 转 `[{type:"text",text:<output>}]`；没有输出则 `[]`。`structured` 固定 `{}`，除非 tool 有结构化结果。 |
| `provider.executed` | Loom 本地 executor 为 `false`；仅在模型提供商实际执行 native tool 时为 `true`。metadata 首期省略。 |
| `snapshot` / `files` / `outputPaths` | 首期省略；实现真实 snapshot、相对文件路径或产物收集后才填写。 |

### 2.3 状态机规则

同一个 run 的 durable 顺序至少为：`prompted → prompt.admitted → step.started → block.started → block.ended → step.ended|step.failed`。text/reasoning/tool block 都按 ID 跟踪；start 后才能 delta/end；每个 tool call 只有一个 terminal success/failed；一个 assistant message 的 step settlement 二选一。取消和 provider error 必须先关闭已打开的 block，之后才发 `step.failed`。

## 3. 目标模块与接口

| 文件 | 职责 |
| --- | --- |
| `apps/server/src/v2_event.rs`（新） | 强类型 `V2Event`、`V2Durable`、32 个 event kind、builder、wire serializer。 |
| `apps/server/src/v2_session_log.rs`（新） | 逐 session sequence 分配、memory index、cursor/replay、watermark 算法。 |
| `apps/server/src/v2_translator.rs`（新） | ReAct/handler 生命周期转成已支持的 v2 typed input；维护 block/tool run tracker。 |
| `apps/server/src/storage.rs` | v2 durable append/page/delete/load；生产持久化实现。 |
| `apps/server/src/state.rs` | v2 bus、log、sequence、publisher 与 session delete cascade；不改 legacy `GlobalEvent`。 |
| `apps/server/src/sse.rs` | v2 global/session stream；严格 cursor parser；仅 comment keepalive。 |
| `apps/server/src/agent_runner.rs` | 同一 `TypedAnyStreamEvent` 同时喂给 legacy translator 和 v2 translator。 |
| `apps/server/src/handlers/session.rs` | admission、model validation、`V2RunContext` 创建、session capability 行为。 |
| `apps/server/tests/v2_session_sse.rs`（新） | schema、log、replay/live race、wire、隔离、双轨集成测试。 |

核心 API（同步 store 沿用现有 trait；若以后异步化，必须以每 session actor/mutex 保留事务边界）：

```rust
pub struct V2RunContext {
    session_id: String,
    assistant_message_id: String,
    agent: String,
    model: V2ModelRef,
    text: HashMap<String, TextTracker>,
    reasoning: HashMap<String, ReasoningTracker>,
    tools: HashMap<String, ToolTracker>,
}

pub fn publish_v2_durable(state: &AppState, input: V2DurableInput)
    -> Result<V2Event, V2PublishError>;
pub fn publish_v2_live(state: &AppState, input: V2LiveInput)
    -> Result<V2Event, V2PublishError>;

fn append_v2_session_event(&self, event: &V2Event) -> Result<(), StoreError>;
fn load_v2_session_events_after(&self, session_id: &str, after: Option<u64>, limit: usize)
    -> Result<V2EventPage, StoreError>;
fn delete_v2_session_events(&self, session_id: &str) -> Result<(), StoreError>;
```

`V2DurableInput` 是 enum（一个 variant 对应一个 durable type），而非 `serde_json::Value`；builder 只接受满足本文件字段规则的结构体。这样 schema 字段名、version 和 durable/live 属性不能被调用方遗漏。

## 4. 存储、原子性与 replay

### 4.1 必须实现的存储语义

v2 durable log 是 session 分区的 append-only log，不能复用 512 条的 legacy global `event_buffer`。生产实现采用 `<loom data dir>/v2-session-events/<sessionID>.jsonl`：每行一个完整 envelope；再以同目录 `<sessionID>.meta.json` 保存最后成功的 `seq`。写入流程为临时文件 + 原子 rename（或等效的单事务 SQLite），以保证 event 行和 last-seq 同时可恢复。InMemoryStore 只用于测试。

启动时扫描 durable log：验证 `aggregateID`、严格递增 `seq` 和 schema fixture 形状，恢复 `next_seq = max_seq + 1`。尾部损坏行不得悄悄 broadcast；记录诊断后拒绝该 session 的 v2 stream，直到修复或显式迁移。删除 session 在删除 session 记录的同一事务/锁范围内删除其 `.jsonl` 与 `.meta.json`，并清空 memory index。

### 4.2 发布事务

```text
per-session publish lock
  读取 next_seq，构建带 seq 的 durable envelope
  append durable store（失败：不改变内存、不 broadcast）
  更新 memory log 和 next_seq
  broadcast 到 v2_event_tx
释放锁
```

不能在锁内 `await`。broadcast receiver lag 不影响 durable correctness：客户端重连后从其最后收到的 `seq` replay。memory cache 仅为优化；权威来源是 store。

### 4.3 session SSE 无竞态算法

1. 订阅 `v2_event_tx`，记录 subscription 后的 session watermark `W`。
2. 从 store 读取 `seq > after` 且 `seq <= W` 的 replay，按 seq 输出。
3. 消费 live bus，仅接受目标 `aggregateID`；仅输出 `seq > max(W, after)`，并将最近输出 seq 前移。
4. receiver lag 或 store page `has_more` 时重新从 store 以最近 seq 读取，绝不跳号或重复。

`after` 必须是十进制非负 `u64`；缺失等同 0；非法/负数/溢出返回 400。默认 page size 500、最大 1,000；SSE stream 在有更多历史页时持续 page，不把 page boundary 暴露成丢失历史。

## 5. 分阶段实施与验收

### PR-0：冻结基准和测试资产

- 将 OpenCode `b8142c7aa` 与 `event.ts` / `session-event.ts` 的路径写入测试注释。
- 新增所有 **32 个** fixture：28 durable + 4 live；每个 fixture 同时断言外层 envelope、data 必填字段与 durable version/缺失规则。
- 建立 decode harness（可使用以该 revision 生成的 JSON schema 或等价 Rust structural validator）；fixture 不依赖 Loom 当前输出。

验收：错误字段名、错误 version、把 delta 标 durable 或漏 sessionID 都能让测试失败。

### PR-1：domain model 与 durable log

- 创建 `v2_event`、`v2_session_log`，实现 typed builder、独立 ID、sequence allocator、production file store、InMemoryStore。
- 扩展 StoreTrait 为 `Result` 返回值；不改现有 `push_event/load_events` 行为。
- 加载、损坏日志、delete cascade 与并发双 session 测试。

验收：重启后同 session 从 `max+1` 延续；A/B session 各自为 `1,2,...`；append 失败不广播；delete 后 replay 为空。

### PR-2：SSE transport

- `/api/session/:id/event` 改读 v2 durable log；移除其 legacy event filter 和所有 JSON heartbeat。
- `/api/event` 改订阅 `v2_event_tx`；`/global/event`、`/event` 不动。
- 统一 `event: message` wire，保留 Axum SSE comment keepalive（不携带 JSON data）。

验收：原始 frame 断言 content type、`event: message`、data JSON、comment keepalive；after 边界、replay/live race、lag recovery、跨 session 隔离全部通过。

### PR-3：prompt、step、text、reasoning（首个可用切片）

- `prompt_v2()` 在 admission 前发布 prompted、持久 user message 成功后发布 admitted；两者共用同一 prompt/message/delivery。
- 先解析和验证 model；无 model 返回 409，且不创建不完整 v2 run。
- `TurnStart` 发 step.started V1；`TurnFinish` 发 step.ended V2；`ProviderError`/取消发 step.failed V2。
- Text/Reasoning start 创建 tracker，end 使用累计全文；所有开块在结束/失败路径被显式收束。
- 同一上游 stream 同时调用现有 legacy translator 与 v2 translator，二者不共享 mutable part state。

验收：成功、provider error、取消、空块、交错 text/reasoning、多 block run；9 个 fixture decode；legacy `message.*`/`session.status` 回归不变。

### PR-4：四种 live delta

- Text 和 reasoning 原始 chunk 通过 `publish_v2_live()` 到 `/api/event`；不会 append、没有 durable、不会出现在 `/api/session/:id/event`。
- Tool input delta 仅当 provider 暴露原始参数 chunk 时发布；当前无来源时 capability 保持 disabled。
- Compaction delta 仅在真实摘要流实现后发布。

验收：慢客户端和高频 token 测试证明 durable log 数量不随 delta 数量增长；重连只靠 ended 恢复完整内容。

### PR-5：tool 全生命周期（7 types）

- `ToolCall`：input.started → input.ended → called；调用 ID 为空时上游生成稳定 ID 后才执行。
- `ToolOutput`：按“状态变化、最多每 250 ms、或累计 8 KiB”发 durable progress，避免 stdout 每 chunk 落盘。
- `ToolEnd` 成功发 success；`ToolError` 或 error end 发 failed；二者互斥。
- 使用第 2.2 节固定的 input/content/provider policy；真实 output path/structured result 出现时才扩展字段。

验收：多 tool、并发 call、无输出、JSON 非 object input、异常、重复上游 end 均有测试；每 call 恰好一个 terminal event。

### PR-6：产品能力事件（12 durable + 1 live）

按真实功能落地次序实现，任一功能未实现则不发布其 event：

| 能力组 | event | 发布位置和 gate |
| --- | --- | --- |
| session 变更 | agent.switched、model.switched、moved、context.updated、synthetic | 成功提交 agent/model/location/context/synthetic 状态后；以触发 message ID 或真实 location 为准。 |
| shell | shell.started、shell.ended | `run_shell` 前/完成后；同一 callID，输出为完整文本。 |
| retry | retried | 每次真实重试调度后；填 attempt、message、isRetryable，HTTP 信息只有确有来源才填。 |
| compaction | started、delta、ended | compaction 事务开始/流式摘要/提交后；reason 为 auto/manual，recent 为最终 recent window。 |
| revert | staged、cleared、committed | 完整 Revert.State 写入、清除、提交成功之后。 |

验收：每一行独立 fixture、publisher、replay、失败不发布与 legacy 回归；不能用 stub endpoint 来绿测。

### PR-7：history、迁移与发布

- `GET /api/session/:id/history` 复用同一 v2 durable store、相同 aggregate sequence 和 exclusive cursor，支持分页；禁止从旧 global buffer 拼装。
- 为已有 session 选择明确迁移策略：没有 v2 log 的旧 session 从首次 v2 event seq=1 开始，不回填伪历史；history 返回空而不是 legacy 事件。
- feature flag 默认关闭，预发布环境进行真实 OpenCode TUI 和 SDK v2 回归；达到所有 gate 后默认开启。

## 6. Capability 表与最终 32 类型完成判定

| 组 | 类型数 | PR | 现阶段承诺 |
| --- | ---: | --- | --- |
| prompt + step + text/reasoning durable | 9 | PR-3 | 首个可用切片 |
| live delta | 4 | PR-4 / PR-6 | text/reasoning 首先；tool input 和 compaction 有真实 source 后启用 |
| tool durable（不含 input delta） | 6 | PR-5 | 完整 lifecycle |
| agent/model/move/context/synthetic/shell/retry | 8 | PR-6 | 随相应产品能力 |
| compaction/revert durable（不含 delta） | 5 | PR-6 | 随事务性能力 |
| **合计** | **32** | — | 28 durable + 4 live-only，和 OpenCode `SessionEvent.Definitions` 一致。 |

发布 checklist 以 [类型矩阵](LOOM-V2-SSE-TYPE-MATRIX.md) 的 32 行为唯一计数来源。

## 7. 测试矩阵与发布门槛

| 层级 | 必测项目 |
| --- | --- |
| schema fixture | 32 类型逐个 decode；required/optional、field casing、durability version。 |
| event model | ID 独立性、finite numbers、model/error/tool 转换、非法 builder 输入拒绝。 |
| durable log | append failure、重启、删除、分页、cursor、损坏尾部、并发 session。 |
| translator | 每个上游 StreamEvent 的顺序、block/tool 状态机、error/cancel 收束。 |
| SSE endpoint | strict `after`、replay/live watermark、lag recovery、session isolation、无 JSON heartbeat。 |
| wire | `event: message`、JSON data、content type、comment keepalive。 |
| compatibility | 旧 global event、legacy translator、现有 TUI 行为不变；OpenCode SDK v2 `/global/event` 和 session event client 可消费。 |
| E2E | real provider text/reasoning/tool/failure/cancel；OpenChamber `/global/health` 和路由代理回归。 |

每个 PR 至少运行 `cargo fmt --check`、`cargo test -p loom-server`、对应 integration tests 和 `scripts/check-protocol.ps1`。在把该脚本列为 release gate 前，先修正它与 handler 当前 session delete `200 {"success":true}` 契约的冲突。完成 PR-7 前不得宣称“v2 SSE 完整兼容”。

## 8. 回滚与可观测性

feature flag 分为 `v2_session_sse`、`v2_global_sse`、`v2_live_delta`；每个默认关闭且独立启用。回滚只关闭 v2 publisher/endpoint 数据源，绝不改变 legacy `state::emit()`、`event_tx`、`event_buffer`、`/global/event` 或已经落盘的 v2 log。

最少监控：append failure、publish latency、每 session sequence gap、receiver lag、replay page count、schema validation failure、每类 event 计数。告警中的 event data 必须脱敏，不记录 prompt、tool output 或 provider 原始错误全文。

禁止的“回滚”是把 `/api/session/:id/event` 改回发送 legacy part/heartbeat 的假 v2 数据；那会掩盖协议错误且破坏 SDK v2 consumer。
