# Loom Server SSE v2 API 规范

状态：当前 v2 wire contract。更新日期：2026-07-24。

## 1. 端点

| 端点 | 内容 | 重放 |
| --- | --- | --- |
| `GET /api/event` | 所有 session 的 v2 durable + live event | 否 |
| `GET /api/session/:sessionID/event?after=<seq>` | 目标 session 的 durable event | 是 |

每一帧显式使用 `event: message`：

```text
event: message
data: {"id":"evt_v2_1","type":"session.next.text.ended","durable":{"aggregateID":"sess_1","seq":8,"version":1},"data":{...}}
```

`/api/*` 不注入 legacy `server.connected` 或 `server.heartbeat` 业务事件。

## 2. Envelope

```ts
type V2Event = {
  id: string
  metadata?: Record<string, unknown>
  type: string
  durable?: { aggregateID: string; seq: number; version: 1 | 2 }
  location?: { directory: string; workspaceID?: string }
  data: Record<string, unknown>
}
```

durable event 必须满足 `durable.aggregateID === data.sessionID`；每个 session 的 `seq` 从 1 开始严格递增。可选字段应缺席，不得写 `null`。

## 3. 全部返回 data schema（32 个 type）

以下类型均隐式交叉 `Base`：

```ts
type Base = { timestamp: number; sessionID: string }
type UnknownError = { type: "unknown"; message: string }
type ModelRef = { id: string; providerID: string; variant?: string }
type LocationRef = { directory: string; workspaceID?: string }
type Tokens = { input: number; output: number; reasoning: number; cache: { read: number; write: number } }
type ProviderMetadata = Record<string, Record<string, unknown>>
type Provider = { executed: boolean; metadata?: ProviderMetadata }
type ToolContent = { type: "text"; text: string } | { type: "file"; uri: string; mime: string; name?: string }
type Prompt = { text: string; files?: { uri: string; mime: string; name?: string; description?: string; source?: { start: number; end: number; text: string } }[]; agents?: { name: string; source?: { start: number; end: number; text: string } }[] }
type Delivery = "steer" | "queue"
```

### 3.1 通用、prompt、shell、retry（10）

```ts
"session.next.agent.switched": Base & { messageID: string; agent: string }
"session.next.model.switched": Base & { messageID: string; model: ModelRef }
"session.next.moved": Base & { location: LocationRef; subdirectory?: string }
"session.next.prompted": Base & { messageID: string; prompt: Prompt; delivery: Delivery }
"session.next.prompt.admitted": Base & { messageID: string; prompt: Prompt; delivery: Delivery }
"session.next.context.updated": Base & { messageID: string; text: string }
"session.next.synthetic": Base & { messageID: string; text: string }
"session.next.shell.started": Base & { messageID: string; callID: string; command: string }
"session.next.shell.ended": Base & { callID: string; output: string }
"session.next.retried": Base & { attempt: number; error: { message: string; isRetryable: boolean; statusCode?: number; responseHeaders?: Record<string, string>; responseBody?: string; metadata?: Record<string, string> } }
```

### 3.2 Step、text、reasoning（9）

```ts
"session.next.step.started": Base & { assistantMessageID: string; agent: string; model: ModelRef; snapshot?: string }
"session.next.step.ended": Base & { assistantMessageID: string; finish: string; cost: number; tokens: Tokens; snapshot?: string; files?: string[] }
"session.next.step.failed": Base & { assistantMessageID: string; error: UnknownError }
"session.next.text.started": Base & { assistantMessageID: string; textID: string }
"session.next.text.delta": Base & { assistantMessageID: string; textID: string; delta: string }
"session.next.text.ended": Base & { assistantMessageID: string; textID: string; text: string }
"session.next.reasoning.started": Base & { assistantMessageID: string; reasoningID: string; providerMetadata?: ProviderMetadata }
"session.next.reasoning.delta": Base & { assistantMessageID: string; reasoningID: string; delta: string }
"session.next.reasoning.ended": Base & { assistantMessageID: string; reasoningID: string; text: string; providerMetadata?: ProviderMetadata }
```

### 3.3 Tool（7）

```ts
"session.next.tool.input.started": Base & { assistantMessageID: string; callID: string; name: string }
"session.next.tool.input.delta": Base & { assistantMessageID: string; callID: string; delta: string }
"session.next.tool.input.ended": Base & { assistantMessageID: string; callID: string; text: string }
"session.next.tool.called": Base & { assistantMessageID: string; callID: string; tool: string; input: Record<string, unknown>; provider: Provider }
"session.next.tool.progress": Base & { assistantMessageID: string; callID: string; structured: Record<string, unknown>; content: ToolContent[] }
"session.next.tool.success": Base & { assistantMessageID: string; callID: string; structured: Record<string, unknown>; content: ToolContent[]; outputPaths?: string[]; result?: unknown; provider: Provider }
"session.next.tool.failed": Base & { assistantMessageID: string; callID: string; error: UnknownError; result?: unknown; provider: Provider }
```

### 3.4 Compaction、revert（6）

```ts
"session.next.compaction.started": Base & { messageID: string; reason: "auto" | "manual" }
"session.next.compaction.delta": Base & { messageID: string; text: string }
"session.next.compaction.ended": Base & { messageID: string; reason: "auto" | "manual"; text: string; recent: string }
type FileDiff = { path: string; status: "added" | "modified" | "deleted"; additions: number; deletions: number; patch: string }
type RevertState = { messageID: string; partID?: string; snapshot?: string; diff?: string; files?: FileDiff[] }
"session.next.revert.staged": Base & { revert: RevertState }
"session.next.revert.cleared": Base
"session.next.revert.committed": Base & { messageID: string }
```

## 4. Replay

`after` 是十进制非负 `u64`；省略等于 `0`，非法、负数或溢出返回 `400 Bad Request`。session stream 只返回 `seq > after` 的 durable record，绝不返回 live delta。

服务端先订阅 live bus，再取得 watermark，随后回放 `after < seq <= watermark`，最后交付 `seq > max(after, watermark)` 的后续 event。由此回放与 live 之间不会丢失或重复。客户端持久化最后成功应用的 `durable.seq`，并将其作为下一次 `after`。

## 5. Live 与 durable

`session.next.text.delta`、`reasoning.delta`、`tool.input.delta` 和 `compaction.delta` 是 live-only，无 `durable`，不会写入 session replay。客户端以 delta 增量渲染，但重连时必须使用 durable `*.ended` 和 terminal event 重建状态。

`/api/event` 可以混合多个 session；客户端必须按 `data.sessionID` 分区。session stream 仅允许目标 session 的 durable sequence。

## 6. 工具生命周期

同一 `assistantMessageID + callID` 的顺序为：

```text
tool.input.started → tool.input.delta* → tool.input.ended
→ tool.called → tool.progress* → tool.success | tool.failed
```

streaming provider 在工具名首次可知时立即发送 `input.started`；`input.ended.text` 是完整参数 JSON 文本，`called.input` 是同一参数 object。每个 call 只能有一个 terminal，terminal 后不得再出现 delta/progress。

没有 provider tool delta 的 client 降级为 `started → delta(完整 JSON) → ended → called`，不会伪造更早发生的工具位置。
