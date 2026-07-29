# Loom v2 Session SSE 修正方案

状态：待实施设计。更新日期：2026-07-24。

目标：让 `GET /api/session/:sessionID/event` 兼容 OpenCode `b8142c7aa` 的 durable `SessionEvent`，同时不破坏当前 TUI/OpenChamber 使用的 legacy `message.*` global event 流。详细差距见 [v2 SSE 兼容规范](V2-SSE-COMPATIBILITY.md)。

## 1. 架构决策

采用双轨事件管线，不修改已有 `state::emit()` 的 legacy 行为。

```text
agent StreamEvent
 ├─ LegacyPartTranslator ──> GlobalEvent bus ──> /global/event
 │                                           └─ 当前 TUI / OpenChamber
 └─ V2SessionEventTranslator ──> V2 event bus ──> /api/event
                                             └─ durable session log
                                                  └─ /api/session/:id/event
```

原因：当前 TUI 虽使用 v2 SDK，实际订阅的是 `GET /global/event`；OpenCode 自己也通过 event-v2 bridge 将 v2 event 投影到全局 bus。相反，session SSE endpoint 的 schema 只允许 durable event，不能把 text/reasoning delta 或 server heartbeat 放进去。

## 2. 当前问题

| 当前组件 | 问题 | 修正 |
| --- | --- | --- |
| `state::emit()` | 生成 `GlobalEvent`，没有 durable metadata，ID 是进程级 `evt_<counter>`。 | 新增 v2 发布 API，不改变 legacy API。 |
| `event_buffer` | 跨 session 的 512 条 ring buffer，`after` 是 event ID。 | 改为按 session 的 durable aggregate log 与单调 sequence。 |
| `api_session_event_stream` | 重放普通 GlobalEvent，并注入 `server.connected/heartbeat`。 | 仅输出该 session 的 typed durable v2 event。 |
| `translator.rs` | 将生命周期折叠为可变 part。 | 保留；并行增加 v2 lifecycle translator。 |
| `StoreTrait` | 只保存 GlobalEvent。 | 新增 v2 session event 的 append/page/delete API。 |

## 3. 类型和状态

新建 `V2Event`，序列化为 OpenCode 通用 Event payload：

```rust
struct V2Event {
    id: String,
    event_type: String, // serde => "type"
    data: serde_json::Value,
    location: Option<LocationRef>,
    metadata: Option<serde_json::Value>,
    durable: Option<V2Durable>,
}

struct V2Durable {
    aggregate_id: String, // serde => "aggregateID"; sess_*
    seq: u64,
    version: u32,
}
```

在 `AppState` 增加、但不替换现有 `event_tx/event_buffer`：

```rust
v2_event_tx: broadcast::Sender<V2Event>,
v2_session_events: RwLock<HashMap<String, VecDeque<V2Event>>>,
v2_next_seq: RwLock<HashMap<String, u64>>,
```

约束：`v2_session_events` 只保存 `durable.is_some()` 的事件；同一 session 的 `seq` 从 1 严格递增；session 删除时级联删除其 v2 log。容量和保留期限必须配置化，不能沿用全局 512 条 ring 的隐式语义。

## 4. 发布与持久化

新增两条受控 API：

```text
publish_v2_live(state, definition, data, location, metadata)
publish_v2_durable(state, definition, session_id, data, location, metadata)
```

`definition` 必须为 Rust enum/常量，固定 event `type` 和 durable version；不接受任意字符串。`publish_v2_live` 只广播。`publish_v2_durable` 必须按以下顺序执行：

1. 分配 per-session `seq`；
2. 写入持久 Store；
3. 更新内存 replay；
4. 广播到 v2 bus。

失败时不广播半成品。扩展 `StoreTrait`：

```rust
fn append_v2_session_event(&self, event: &V2Event);
fn load_v2_session_events_after(&self, session_id: &str, after: Option<u64>, limit: usize)
    -> V2EventPage;
fn delete_v2_session_events(&self, session_id: &str);
```

将来 `GET /api/session/:id/history` 复用同一分页读取，避免 history 与 SSE replay 出现不同顺序。

## 5. Session SSE handler

把 `sse::api_session_event_stream` 改为：

1. 验证 session 存在；不存在返回目标 schema 的 not-found error。
2. 将 `after` 解析为非负 aggregate sequence；无效值返回 400，不能“找不到 cursor 就全量 replay”。
3. 重放此 session `seq > after` 的 durable page。
4. 订阅 `v2_event_tx`，只转发 `durable.aggregateID == sessionID` 的新 event。
5. 只发送 `V2Event` data；禁止发送 `server.connected` 或 `server.heartbeat` 业务 event。
6. 可以保留 SSE comment keepalive。

为避免 replay 与 live subscription 的竞态，必须在 subscription watermark 下建立订阅并按 `seq` 去重；不能先读 replay、再订阅 broadcast。

## 6. Translator 双发

现有 `translator.rs::translate_and_emit()` 不改，继续生成 legacy `message.part.updated`。新增 `V2SessionEventTranslator`，持有：

```rust
struct V2RunContext {
    session_id: String,
    assistant_message_id: String,
    agent: String,
    model: ModelRef,
    text_blocks: HashMap<BlockId, String>,
    reasoning_blocks: HashMap<BlockId, ReasoningState>,
    tools: HashMap<CallId, ToolState>,
}
```

必须通过 block ID/call ID 路由，不能以“最近 part 类型”推断状态。

| Loom 输入 | v2 输出 | durable |
| --- | --- | --- |
| v2 prompt admission | `prompted`、`prompt.admitted` | v1 |
| `TurnStart` | `step.started` | v1 |
| `TextBlockStart/Delta/End` | `text.started/delta/ended` | started/ended v1；delta live-only |
| `ReasoningBlockStart/Delta/End` | `reasoning.started/delta/ended` | started/ended v1；delta live-only |
| `ToolCall` | `tool.input.*`、`tool.called` | input boundary/called v1；input delta live-only |
| `ToolOutput` | `tool.progress` | v1，限频 |
| `ToolEnd` | `tool.success` 或 `tool.failed` | v1 |
| `TurnFinish` | `step.ended` | v2 |
| `ProviderError` | `step.failed` | v2 |

尚未有底层能力的 agent/model switch、shell、retry、compaction、revert 不发布成功 v2 event；相应 HTTP handler 也不得以空响应伪装支持。

## 7. 字段补齐

| 字段 | 方案 |
| --- | --- |
| `timestamp` | 每次 v2 publish 写 UTC epoch ms，不复用 part 的 `time`。 |
| `assistantMessageID` | 从 `V2RunContext` 提供，不从 part JSON 反推。 |
| `agent/model` | 从 session 当前选择取得；缺 model 时先补模型选择链路。 |
| `finish/cost` | 显式映射当前 `reason`；cost 必须接入 provider meter，不能伪造完整兼容。 |
| token | 保持当前 `input/output/reasoning/cache.read/cache.write`，所有值为有限数值。 |
| provider metadata | 建 mapper；无法保真时省略 optional 字段。 |
| `ToolContent[]` | 建 content adapter，禁止把裸 output string 填入 schema array。 |
| `UnknownError` | 将现有 `{name,data}` 错误转换为目标 schema，不再复用 `session.error`。 |

## 8. 实施阶段

### Phase A：事件基础设施

- `V2Event`、per-session sequence、Store API、v2 event bus。
- durable replay/history 读取。
- session SSE handler 切到 v2 log，并移除 session stream 的 server lifecycle business event。

验收：fixture replay、跨 session 隔离、`after` 边界和 restart reload 全部通过。

### Phase B：核心 LLM 生命周期

- prompt、step、text、reasoning、provider failure 的 v2 translator。
- legacy part event 双发不变。

验收：断线后可由 durable ended/failed/step event 恢复状态。

### Phase C：工具与成本

- tool input/called/progress/success/failed。
- cost、snapshot/files、provider metadata。

验收：真实工具调用 payload 能由固定 OpenCode schema fixture decode。

### Phase D：补充生命周期

- shell、retry、compaction、revert、agent/model switch。
- history endpoint 复用同一 aggregate sequence。

## 9. 测试门禁

1. 每个已支持 v2 event 都有 valid/invalid schema fixture。
2. translator 单测断言 type、data、durable version 和 sequence。
3. replay 集成测试覆盖多 session、`after`、断线重连、持久 reload 和慢订阅者。
4. wire 测试覆盖 content type、SSE `message` event、JSON decode、keepalive，且 session SSE 不含 server lifecycle business event。
5. dual-emission 回归确认 legacy global stream 仍包含 `message.updated`、`message.part.updated`、`session.status`。
6. 真实 Provider E2E 覆盖 text、reasoning、tool、失败和取消。

只有固定 OpenCode revision 的 `SessionEvent.Durable` 能 decode 所有已声明支持的 payload，且 replay、session 隔离与 legacy TUI 回归都通过，才能标记 v2 session SSE 兼容完成。
