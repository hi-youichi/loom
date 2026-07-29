# OpenCode v2 Session SSE 兼容规范

状态：当前差距说明与实施规范。最后核对：2026-07-24。

本文定义 Loom 要兼容 OpenCode **v2 session SSE** 时的目标，不把当前 OpenChamber/TUI 的全局事件消费误称为完整 v2 session-event 兼容。

## 1. 权威来源与范围

| 项目 | 来源 |
| --- | --- |
| OpenCode revision | `C:\Users\heycj\dev\opencode` 的 `b8142c7aa` |
| v2 事件 schema | `packages/schema/src/session-event.ts` |
| 通用 Event envelope | `packages/schema/src/event.ts` |
| session SSE endpoint | `packages/protocol/src/groups/session.ts` |
| Loom 当前实现 | `apps/server/src/{sse,state,translator,agent_runner}.rs` |

范围仅包括 `GET /api/session/:sessionID/event` 的 v2 session event stream。`GET /global/event` 是当前 TUI 的实际订阅通道，使用 `GlobalEvent`，不在此 endpoint 的 typed session-event 兼容范围内。

## 2. 目标端点与 wire contract

```text
GET /api/session/:sessionID/event?after=<exclusive-sequence>
Accept: text/event-stream
```

OpenCode 的 endpoint 要求：

- 先重放该 session 在 `after` 之后的 **durable** 事件，再持续输出新 durable 事件。
- `after` 的语义是该 session aggregate 的排他 sequence，不是服务器进程内随机/全局 event ID。
- stream payload 必须属于 `SessionEvent.Durable` 联合类型。
- 事件遵循通用 `Event.Payload` 形状：

```json
{
  "id": "evt_...",
  "type": "session.next.step.ended",
  "durable": { "aggregateID": "sess_...", "seq": 42, "version": 2 },
  "location": { "directory": "..." },
  "data": {
    "timestamp": 1760000000000,
    "sessionID": "sess_...",
    "assistantMessageID": "msg_...",
    "finish": "stop",
    "cost": 0,
    "tokens": {
      "input": 10,
      "output": 20,
      "reasoning": 0,
      "cache": { "read": 0, "write": 0 }
    }
  }
}
```

OpenCode server-side SSE handlers使用 SSE `message` event。Loom v2 stream 现在以 Axum `Event::event("message").data(...)` 发送 JSON data frame；仍需真实 SDK/TUI 抓包作为最终互操作验收。

## 3. 当前 Loom 行为

Loom 已具备三条 SSE 路由：

- `GET /global/event`：v1-style wrapper。
- `GET /api/event`：v2 global bus（durable + live）。
- `GET /api/session/:id/event`：从 `$LOOM_HOME/server/v2-events` 的 per-session durable log 按 sequence 重放。

`GlobalEvent` 的 v2 JSON 外壳已接近通用 Event envelope：`id`、`type`、可选 `metadata/durable/location`、`data`。但由 `emit()` 创建的事件目前没有 `durable`，event ID 为进程内的 `evt_<counter>`，重放 ring buffer 上限为 512。

Translator 将 ReAct `StreamEvent` 映射为：

- `TextBlockStart/Delta/End` → `message.part.updated` text part；delta 是累计文本更新。
- `ReasoningBlockStart/Delta/End` → `message.part.updated` reasoning part。
- `TurnStart/TurnFinish` → `message.part.updated` 的 `step-start/step-finish` part。
- `Tool*` → 同一个 tool part 的状态和 output 更新。
- `ProviderError` → `session.error`。

生产 translator 双发 legacy part 与 v2 `session.next.step/text/reasoning/tool.*`；text/reasoning delta 是 `/api/event` 的 live-only event。session handler 还发布 prompt、agent/model/moved、shell、compaction/context 和 revert event。

## 4. 事件映射与差距

### 4.1 通用、prompt、shell

| OpenCode v2 event | Loom 当前对应 | 差距 |
| --- | --- | --- |
| `agent.switched` | v2 agent handler | 已发 durable；待 E2E。 |
| `model.switched` | v2 model handler | 已发 durable `Model.Ref`；待 E2E。 |
| `moved` | PATCH directory | 已发 durable location。 |
| `prompted` | v2 prompt admission | 已发 durable。 |
| `prompt.admitted` | v2 prompt admission | 已发 durable。 |
| `context.updated` / `synthetic` | compaction / 无入口 | context 已发；synthetic 尚无真实入口。 |
| `shell.started` / `shell.ended` | shell handler | 已发 durable call lifecycle。 |
| `retried` | 无 retry pipeline | 尚无真实来源。 |

### 4.2 Step

| OpenCode v2 event | Loom 当前表示 | 必须补齐 |
| --- | --- | --- |
| `step.started` | `message.part.updated`，part type=`step-start` | 独立事件、`timestamp`、`assistantMessageID`、`agent`、`model`、可选 `snapshot`、durable v1。 |
| `step.ended` | `message.part.updated`，part type=`step-finish` | 独立事件、`assistantMessageID`、`finish`（当前为 `reason`）、`cost`、可选 snapshot/files、durable v2。token 名称已接近目标。 |
| `step.failed` | `session.error` | `UnknownError`、assistant message ID、durable v2。 |

### 4.3 Text 与 reasoning

| OpenCode v2 event | Loom 当前表示 | 必须补齐 |
| --- | --- | --- |
| `text.started` | 空 text part updated | `textID`、assistant message ID、typed durable event。 |
| `text.delta` | 累计 text part updated | 真实增量 `delta`、`textID`；该事件本身应为 live-only。 |
| `text.ended` | text part updated，含终止 time | 完整 `text`、`textID`、typed durable event。 |
| `reasoning.started` | 空 reasoning part updated | `reasoningID`、`providerMetadata`、typed durable event。 |
| `reasoning.delta` | 累计 reasoning part updated | 真实增量 `delta`、reasoning ID；live-only。 |
| `reasoning.ended` | reasoning part updated，含终止 time | 完整 `text`、`reasoningID`、`providerMetadata`、typed durable event。 |

### 4.4 Tool

| OpenCode v2 event | Loom 当前表示 | 必须补齐 |
| --- | --- | --- |
| `tool.input.started/delta/ended` | 无独立事件 | input 生命周期和 raw input 边界。 |
| `tool.called` | tool part `pending` | `callID`、`assistantMessageID`、`tool`、结构化 `input`、`provider.executed/metadata`。 |
| `tool.progress` | tool part output 累加 | `structured` 和 `content: ToolContent[]`，且应限频 durable。 |
| `tool.success` | tool part `completed` | `structured`、`content`、可选 outputPaths/result、provider metadata。 |
| `tool.failed` | tool part `error` 或 `session.error` | `UnknownError`、可选 result、provider metadata。 |

### 4.5 其余 session lifecycle

Loom 当前没有 v2 typed 实现：`compaction.started/delta/ended`、`revert.staged/cleared/committed`。它们不应由空 event 或 2xx stub 冒充已支持。

## 5. Durability 与 replay

OpenCode 将事件分为两类：

- durable：可被 `session.events` 重放；大部分 lifecycle event 使用 version 1，step settlement 使用 version 2。
- live-only：`text.delta`、`reasoning.delta`、`tool.input.delta`、`compaction.delta`；重连后不补发，依赖对应 ended 事件恢复完整值。

Loom 当前将所有 `emit()` 事件写入同一个 buffer，并按 `sessionID` 过滤；这与上述语义不同：

- 未标记 durable，也没有每-session `aggregateID/seq/version`。
- 缓冲区为固定 512 条，会跨 session 挤出事件。
- 找不到 `after` ID 时会重放整个 buffer；v2 应使用严格、可解释的 aggregate sequence。
- 当前 session stream 会自行注入 `server.connected` 和 `server.heartbeat`。这两个并非 `SessionEvent.Durable`，不应混入该 endpoint；它们可以保留在 global event stream 或 SSE comment keepalive。

## 6. 与当前 TUI 的关系

当前 OpenCode TUI 使用 `@opencode-ai/sdk/v2`，但在 `packages/tui/src/context/sdk.tsx` 订阅的是 `sdk.global.event()`，其 generated path 为 `GET /global/event`。因此：

- `message.updated`、`message.part.updated`、`session.status` 的 global stream 仍是当前 TUI 主路径。
- 本文的 v2 session SSE 是 SDK/API 完整兼容工作；其缺失不等于当前 TUI 必然无法对话。
- 不应因为 TUI 主路径可渲染，就声称 `GET /api/session/:id/event` 已兼容。

## 7. 实施顺序

1. 固定 OpenCode revision，并为 `SessionEvent.Durable` 生成 Rust fixture/JSON schema 测试数据。
2. 将 durable event store 变为按 `sessionID` 的持久 aggregate log，并分配 `seq`。
3. 为 step/text/reasoning/tool 建立独立 v2 translator；保留当前 part translator 以服务 global/TUI 流。
4. 把 delta 标记为 live-only；确保 reconnect 后由 durable ended/progress/settlement 事件恢复状态。
5. 从 session scoped SSE 移除 `server.connected/heartbeat` 业务事件，并添加完整 endpoint contract test。
6. 实现或明确拒绝 compaction、revert、retry、shell 等事件族。

## 8. 必须覆盖的测试

- 单回合 text：started → 多个 delta → ended，断线后只重放 started/ended。
- reasoning 与 text 交错，按各自 ID 收尾。
- 工具 input、called、progress、success 和 failed 两条路径。
- step ended 与 failed 的 durable version、`seq` 单调性与 token/cost shape。
- `after` 为当前 sequence、旧 sequence、无效 sequence 的行为。
- 两个 session 并发时，replay 和 live stream 绝不串 session。
- SSE frame transport：content type、`event: message`、data decode、keepalive、断线重连。

## 9. 完成定义

只有当目标 OpenCode revision 的 `SessionEvent.Durable` 能对 Loom session SSE 的每个 durable payload 做 schema decode，且真实重连/replay 测试通过时，才能标记“v2 session SSE 兼容”。
