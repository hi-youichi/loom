# Loom Server SSE v1 API 规范

状态：当前 legacy wire contract。更新日期：2026-07-24。

## 1. 端点与帧

`GET /global/event` 返回全局 legacy event stream。每个业务 record 的 data 为：

```text
data: {"directory":"C:/work","payload":{"id":"evt_...","type":"message.part.updated","properties":{...}}}

```

```ts
type LegacyEvent = {
  directory: string
  payload: { id: string; type: string; properties: unknown }
}
```

Axum 未显式写入 `event:` 时，SSE 客户端按默认 `message` event 处理。客户端按 `payload.type` 分派，并从 `payload.properties` 读取事件数据。

## 2. 连接生命周期

连接建立后先收到：

```text
server.connected { version: string }
```

其后每 10 秒收到：

```text
server.heartbeat {}
```

同时服务端发送 SSE comment keepalive；它只服务于 TCP/proxy 保活，与 `server.heartbeat` 业务事件不同。

## 3. 语义和限制

- 这是广播流，无 cursor、无持久化 replay、无 delivery guarantee。
- 重连只能得到重连后的新事件；需要可靠恢复时必须使用 v2 session SSE。
- stream 可包含 `server.*`、`session.*` 与 `message.*`；消费者必须忽略未知 type。
- 工具、text、reasoning 的渲染以 `message.part.updated` 为准。同一 part ID 更新时必须替换/合并现有 part，不能重复插入。

## 4. 工具 part

provider 流式工具调用会在工具名首次可知时创建 `type: "tool"` part；后续 `message.part.updated` 使用同一 `part.id`/`callID` 更新 input、running 状态与结果。因而 part 在 list 中的位置就是工具开始被 provider 声明的时刻，而不是工具执行结束的时刻。

v1 不支持 session replay；不要把 `payload.id` 当作 cursor 或跨连接的顺序保证。

## 5. legacy `message.*` 返回 schema

v1 没有封闭的 `session.next.*` union；所有 payload 都通过 `LegacyEvent.payload.properties` 传输。当前 agent streaming 的核心返回是以下两种：

```ts
type MessagePartUpdated = {
  sessionID: string
  time: number
  part:
    | TextPart
    | ReasoningPart
    | ToolPart
    | StepStartPart
    | StepFinishPart
}
type MessageUpdated = { sessionID: string; info: { id: string; role: "assistant"; finish: "stop" | "cancelled" | "error" } }
type TextPart = { id: string; type: "text"; text: string; time: { start: number; created: number; end?: number } }
type ReasoningPart = { id: string; type: "reasoning"; text: string; time: { start: number; created: number; end?: number } }
type ToolPart = { id: string; type: "tool"; callID: string; tool: string; time: { start: number; end?: number }; state: { status: "pending" | "running" | "completed" | "error"; input: Record<string, unknown>; raw: string; output: string; title: string; metadata: Record<string, unknown>; error?: string; time: { start: number; end?: number } } }
type StepStartPart = { id: string; type: "step-start"; time: { start: number; created: number } }
type StepFinishPart = { id: string; type: "step-finish"; reason: string; tokens: { input: number; output: number; reasoning?: number; cache: { read?: number; write?: number } }; time: { start: number; end: number; created: number; completed: number } }
```

对应 event type 为 `message.part.updated` 与 `message.updated`。

## 6. 全部当前 legacy event 的 `properties` schema

下表是当前 `apps/server/src` 所有会发布到 legacy bus 的 `server.*`、`session.*`、`message.*` type。所有定义都再包一层本文件第 1 节的 `LegacyEvent`。

```ts
type SessionInfo = {
  id: string; slug: string; projectID: string; directory: string; title: string; version: string
  parentID: string | null; workspaceID: string | null; path: string | null
  summary: { additions: number; deletions: number; files: number } | null
  cost: number | null
  tokens: { input: number; output: number; reasoning: number; cache: { read: number; write: number } } | null
  share: { url: string } | null; permission: unknown | null; revert: unknown | null
  agent: string | null; model: { providerID: string; id: string; variant: string | null } | null
  time: { created: number; updated: number; compacting: number | null; archived: number | null }
  metadata?: unknown; [extra: string]: unknown
}
type MessageInfo = {
  id: string; sessionID: string; role: string; time: unknown; agent: string
  model: unknown | null; parentID: string | null; tool: unknown | null; finish: string | null
  providerID: string | null; modelID: string | null; path: unknown | null; cost: number | null
  tokens: unknown | null; mode: string | null; error?: unknown; structured?: unknown
  variant?: string; summary?: boolean; format?: unknown; system?: string; tools?: unknown
}
```

### 6.1 Server 与 session（13）

```ts
"server.connected": { version: string }
"server.heartbeat": {}
"server.config.changed": {}
"server.instance.disposed": {}
"session.created": { sessionID: string; info: SessionInfo }
"session.updated": { sessionID: string; info: SessionInfo }
"session.deleted": { sessionID: string }
"session.init": { sessionID: string }
"session.prompt": { sessionID: string; messageID: string }
"session.status": { sessionID: string; status: { type: "busy" | "idle" } }
"session.error": { sessionID: string; error: { name: string; data: { message: string } } }
"session.summarize": { sessionID: string; [field: string]: unknown }
"location.snapshot": Record<string, unknown> // HTTP request body is forwarded unchanged
```

`session.summarize` 和 `location.snapshot` 是透传/扩展 payload：当前 server 不再收窄其 body；因此 `Record<string, unknown>` 是该接口的完整允许 JSON schema，而不是遗漏字段。

### 6.2 Message 与 part（5）

```ts
"message.updated": { sessionID: string; info: MessageInfo; finish?: "stop" | "cancelled" | "error" }
"message.removed": { sessionID: string; messageID: string }
"message.part.updated": { sessionID: string; part: TextPart | ReasoningPart | ToolPart | StepStartPart | StepFinishPart; time?: number }
"message.part.delta": { sessionID: string; partID: string; delta: string }
"message.part.removed": { sessionID: string; messageID: string; partID: string }
```

`message.part.delta` 仅由 test-support fake runner 产生；生产 agent 使用累积后的 `message.part.updated`。客户端仍必须能解析它，以便测试和兼容消费者不产生分支。

### 6.3 非 legacy type 的排除规则

`session.next.*` 是 v2 flat envelope，绝不应按本文件的 `payload.properties` schema 解析；完整 schema 在 [v2 SSE API 规范](SSE-V2-API-SPEC.md)。`pty.deleted` 是 PTY 专用通知，当前不经 `/global/event` 的 legacy contract 公开，不能将它列为 v1 SSE type。
