# 附录 A：OpenCode v2 Session Event Schema

> 返回 [README.md](README.md)
> 源文件：`packages/schema/src/session-event.ts`

## A.1 基础字段

所有事件共享：

```typescript
const Base = {
  timestamp: DateTimeUtcFromMillis,  // 毫秒时间戳
  sessionID: SessionID,              // "sess_" + nanoid
}
```

## A.2 Step（回合）

```typescript
// session.next.step.started
{
  type: "session.next.step.started",
  timestamp: number,
  sessionID: string,
  assistantMessageID: string,        // "msg_" + nanoid
  agent: string,
  model: Model.Ref,
  snapshot?: string,
}

// session.next.step.ended
{
  type: "session.next.step.ended",
  timestamp: number,
  sessionID: string,
  assistantMessageID: string,
  finish: string,                    // "stop" | "tool_calls" | "length" | ...
  cost: number,
  tokens: {
    input: number,
    output: number,
    reasoning: number,
    cache: { read: number, write: number },
  },
  snapshot?: string,
  files?: string[],
}

// session.next.step.failed
{
  type: "session.next.step.failed",
  timestamp: number,
  sessionID: string,
  assistantMessageID: string,
  error: { name: string, data: { message: string } },
}
```

## A.3 Text（文本 block）

```typescript
// session.next.text.started — 可重放
{
  type: "session.next.text.started",
  timestamp: number,
  sessionID: string,
  assistantMessageID: string,
  textID: string,                    // 唯一标识，等价于 Loom part_id
}

// session.next.text.delta — 仅实时流
{
  type: "session.next.text.delta",
  timestamp: number,
  sessionID: string,
  assistantMessageID: string,
  textID: string,
  delta: string,                     // 增量文本
}

// session.next.text.ended — 可重放
{
  type: "session.next.text.ended",
  timestamp: number,
  sessionID: string,
  assistantMessageID: string,
  textID: string,
  text: string,                      // 完整文本（用于重放）
}
```

## A.4 Reasoning（推理 block）

```typescript
// session.next.reasoning.started — 可重放
{
  type: "session.next.reasoning.started",
  timestamp: number,
  sessionID: string,
  assistantMessageID: string,
  reasoningID: string,               // 唯一标识
  providerMetadata?: ProviderMetadata,
}

// session.next.reasoning.delta — 仅实时流
{
  type: "session.next.reasoning.delta",
  timestamp: number,
  sessionID: string,
  assistantMessageID: string,
  reasoningID: string,
  delta: string,
}

// session.next.reasoning.ended — 可重放
{
  type: "session.next.reasoning.ended",
  timestamp: number,
  sessionID: string,
  assistantMessageID: string,
  reasoningID: string,
  text: string,                      // 完整推理文本
  providerMetadata?: ProviderMetadata,
}
```

## A.5 Tool（工具调用）

```typescript
// session.next.tool.input.started
{ type: "session.next.tool.input.started", ..., callID, name }

// session.next.tool.input.delta — 仅实时流
{ type: "session.next.tool.input.delta", ..., callID, delta }

// session.next.tool.input.ended
{ type: "session.next.tool.input.ended", ..., callID, text }

// session.next.tool.called
{
  type: "session.next.tool.called",
  ..., callID,
  tool: string,
  input: Record<string, unknown>,
  provider: { executed: boolean, metadata?: ProviderMetadata },
}

// session.next.tool.progress
{
  type: "session.next.tool.progress",
  ..., callID,
  structured: Record<string, unknown>,
  content: ToolContent[],
}

// session.next.tool.success
{
  type: "session.next.tool.success",
  ..., callID,
  structured: Record<string, unknown>,
  content: ToolContent[],
  outputPaths?: string[],
  result?: unknown,
  provider: { executed: boolean, metadata?: ProviderMetadata },
}

// session.next.tool.failed
{
  type: "session.next.tool.failed",
  ..., callID,
  error: { name: string, data: { message: string } },
  result?: unknown,
  provider: { executed: boolean, metadata?: ProviderMetadata },
}
```

## A.6 Compaction（上下文压缩）

```typescript
// session.next.compaction.started
{ type: "session.next.compaction.started", ..., messageID, reason: "auto" | "manual" }

// session.next.compaction.delta — 仅实时流
{ type: "session.next.compaction.delta", ..., messageID, text }

// session.next.compaction.ended
{ type: "session.next.compaction.ended", ..., messageID }

// session.next.compaction.failed
{ type: "session.next.compaction.failed", ..., messageID, error }
```

## A.7 其他事件

| 事件 type | 关键字段 |
|---|---|
| `session.next.prompted` | messageID, prompt, delivery |
| `session.next.prompt.admitted` | 同上 |
| `session.next.agent.switched` | messageID, agent |
| `session.next.model.switched` | messageID, model |
| `session.next.context.updated` | messageID, text |
| `session.next.moved` | location, subdirectory? |
| `session.next.retried` | attempt, error: RetryError |

## A.8 Loom 映射对照

| OpenCode v2 事件 | Loom StreamEvent | Loom SSE 事件 | 字段差异 |
|---|---|---|---|
| `session.next.step.started` | `TurnStart` | `message.part.updated` (type: step-start) | Loom 缺 `agent`/`model`/`snapshot` 字段 |
| `session.next.step.ended` | `TurnFinish` | `message.part.updated` (type: step-finish) | OpenCode 有 `cost`/`reasoning`/`cache.read`/`cache.write`；Loom 缺 |
| `session.next.text.started` | `TextBlockStart` | `message.part.updated` (type: text, text: "") | Loom 用 `part.id` 对应 `textID` |
| `session.next.text.delta` | `TextDelta` | `message.part.updated` (累积文本) | OpenCode 发增量 `delta`；Loom 发完整 `part.text` |
| `session.next.text.ended` | `TextBlockEnd` | `message.part.updated` (加盖 time.end) | OpenCode 携带完整 `text`；Loom 不携带（translator 已累积） |
| `session.next.reasoning.started` | `ReasoningBlockStart` | `message.part.updated` (type: reasoning) | OpenCode 有 `providerMetadata`；Loom 用 `metadata` |
| `session.next.reasoning.delta` | `ReasoningDelta` | `message.part.updated` (累积文本) | 同 text.delta |
| `session.next.reasoning.ended` | `ReasoningBlockEnd` | `message.part.updated` (加盖 time.end) | 同 text.ended |
| `session.next.tool.called` | `ToolCall` | `message.part.updated` (type: tool) | OpenCode 有 `provider.executed`；Loom 无 |
| `session.next.tool.success` | `ToolEnd` | `message.part.updated` (status: completed) | OpenCode 有 `outputPaths`/`result`；Loom 用 `result` |
| `session.next.tool.failed` | `ToolError`/`ToolEnd` | `message.part.updated` (status: error) | — |
| `session.next.step.failed` | `ProviderError` | `session.error` | — |

### 已知字段 gap（需后续补齐）

| Gap | OpenCode 字段 | Loom 当前 | 优先级 |
|---|---|---|---|
| Token 结构 | `{ input, output, reasoning, cache: { read, write } }` | `{ prompt, completion, total, cached }` | P1 |
| Step cost | `cost: number` | 不发送 | P2 |
| Step agent/model | `agent`, `model` | 不发送 | P2 |
| Text/Reasoning delta vs full | `delta` 增量 | `part.text` 累积 | P1（X1 任务） |
| Text/Reasoning ended 携带全量 | `text` 完整 | 不携带 | P2 |
