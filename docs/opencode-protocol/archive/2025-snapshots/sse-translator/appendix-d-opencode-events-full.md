# 附录 D：OpenCode v2 Session Event 完整清单

> 返回 [README.md](README.md)
> 源文件：`C:\Users\heycj\dev\opencode\packages\schema\src\session-event.ts`
> 依据：[附录 A](appendix-a-opencode-v2-schema.md) + 本附录（[附录 B](appendix-b-sse-payload-examples.md) 之前已给出 Loom 实际样例）

## D.1 持久化级别

| 级别 | 用途 | 注释 |
|---|---|---|
| `durable.version: 1` | 业务事件（started/ended/called/success/failed） | 可重放 |
| `durable.version: 2`（`stepSettlementOptions`） | `Step.Started` / `Step.Ended` / `Step.Failed` | 结算事件，可重放 |
| 非 durable | `Text.Delta` / `Reasoning.Delta` / `Tool.Input.Delta` / `Compaction.Delta` | **仅实时**，重放时丢弃 |

> OpenCode 注释原文："Stream fragments are live-only; Text.Ended is the replayable full-value boundary."

## D.2 完整事件清单（32 个）

### 通用事件（v1 durable）

| Event `type` | 字段 | Schema 行号 |
|---|---|---|
| `session.next.agent.switched` | `Base`, `messageID`, `agent: string` | L54 |
| `session.next.model.switched` | `Base`, `messageID`, `model: Model.Ref` | L65 |
| `session.next.moved` | `Base`, `location: Location.Ref`, `subdirectory?: RelativePath` | L76 |
| `session.next.prompted` | `Base`, `messageID`, `prompt: Prompt`, `delivery: Delivery` | L87 |
| `session.next.prompt.admitted` | `Base`, `messageID`, `prompt: Prompt`, `delivery: Delivery` | L94 |
| `session.next.context.updated` | `Base`, `messageID`, `text: string` | L101 |
| `session.next.synthetic` | `Base`, `messageID`, `text: string` | L112 |
| `session.next.shell.started` | `Base`, `messageID`, `callID`, `command: string` | L124 |
| `session.next.shell.ended` | `Base`, `callID`, `output: string` | L136 |
| `session.next.retried` | `Base`, `attempt: number`, `error: RetryError` | L387 |

### Step 事件（v2 durable，结算）

| Event `type` | 字段 | Schema 行号 |
|---|---|---|
| **`session.next.step.started`** | `Base`, `assistantMessageID`, `agent: string`, `model: Model.Ref`, `snapshot?: string` | L149 |
| **`session.next.step.ended`** | `Base`, `assistantMessageID`, `finish: string`, `cost: number`, `tokens: { input, output, reasoning, cache: { read, write } }`, `snapshot?`, `files?` | L162 |
| **`session.next.step.failed`** | `Base`, `assistantMessageID`, `error: UnknownError` | L185 |

### Text 事件

| Event `type` | 字段 | 持久 | Schema 行号 |
|---|---|---|---|
| **`session.next.text.started`** | `Base`, `assistantMessageID`, `textID: string` | v1 | L198 |
| **`session.next.text.delta`** | `Base`, `assistantMessageID`, `textID`, `delta: string` | ❌ live-only | L210 |
| **`session.next.text.ended`** | `Base`, `assistantMessageID`, `textID`, `text: string` | v1 | L221 |

### Reasoning 事件

| Event `type` | 字段 | 持久 | Schema 行号 |
|---|---|---|---|
| **`session.next.reasoning.started`** | `Base`, `assistantMessageID`, `reasoningID`, `providerMetadata?` | v1 | L235 |
| **`session.next.reasoning.delta`** | `Base`, `assistantMessageID`, `reasoningID`, `delta: string` | ❌ live-only | L248 |
| **`session.next.reasoning.ended`** | `Base`, `assistantMessageID`, `reasoningID`, `text: string`, `providerMetadata?` | v1 | L259 |

### Tool 事件

| Event `type` | 字段 | 持久 | Schema 行号 |
|---|---|---|---|
| `session.next.tool.input.started` | `Base`, `assistantMessageID`, `callID`, `name` | v1 | L281 |
| `session.next.tool.input.delta` | `Base`, `assistantMessageID`, `callID`, `delta: string` | ❌ live-only | L292 |
| `session.next.tool.input.ended` | `Base`, `assistantMessageID`, `callID`, `text: string` | v1 | L301 |
| **`session.next.tool.called`** | `Base`, `assistantMessageID`, `callID`, `tool: string`, `input: Record<string, unknown>`, `provider: { executed, metadata? }` | v1 | L312 |
| `session.next.tool.progress` | `Base`, `assistantMessageID`, `callID`, `structured: Record`, `content: ToolContent[]` | v1 | L331 |
| **`session.next.tool.success`** | `Base`, `assistantMessageID`, `callID`, `structured: Record`, `content: ToolContent[]`, `outputPaths?`, `result?`, `provider: { executed, metadata? }` | v1 | L342 |
| **`session.next.tool.failed`** | `Base`, `assistantMessageID`, `callID`, `error: UnknownError`, `result?`, `provider: { executed, metadata? }` | v1 | L359 |

### Compaction 事件

| Event `type` | 字段 | 持久 | Schema 行号 |
|---|---|---|---|
| `session.next.compaction.started` | `Base`, `messageID`, `reason: "auto" \| "manual"` | v1 | L399 |
| `session.next.compaction.delta` | `Base`, `messageID`, `text: string` | ❌ live-only | L410 |
| `session.next.compaction.ended` | `Base`, `messageID`, `reason`, `text: string`, `recent: string` | v1 | L420 |

### Revert 事件

| Event `type` | 字段 | 持久 | Schema 行号 |
|---|---|---|---|
| `session.next.revert.staged` | `Base`, `revert: Revert.State` | v1 | L435 |
| `session.next.revert.cleared` | `Base` | v1 | L440 |
| `session.next.revert.committed` | `Base`, `messageID` | v1 | L443 |

## D.3 Source（用于 delta 重放）

```ts
export const Source = Schema.Struct({
  start: NonNegativeInt,
  end: NonNegativeInt,
  text: Schema.String,
})
```

`Source` 携带一个文本片段的字符区间，用于 `.delta` 事件的离线重放定位。

## D.4 关键枚举值

### Step.Ended `finish` 字段

来源 OpenCode `LLMClient` 推断，可能值：
- `"stop"` — 正常完成
- `"tool_calls"` — 需要工具调用
- `"length"` — 达到 max_tokens
- `"content_filter"` — 内容过滤
- 其他 provider-specific 值

### Step.Failed `error: UnknownError`

来源 `SessionMessage.UnknownError`：
```ts
{ name: string, data: { message: string, [key: string]: unknown } }
```

### Token 完整结构（Step.Ended `tokens`）

```ts
tokens: {
  input: number,        // 提示 token
  output: number,       // 生成 token
  reasoning: number,    // 推理专用 token
  cache: {
    read: number,       // 缓存读取命中
    write: number,      // 缓存写入
  }
}
```

### RetryError（Retried `error`）

```ts
{
  message: string,
  statusCode?: number,
  isRetryable: boolean,
  responseHeaders?: Record<string, string>,
  responseBody?: string,
  metadata?: Record<string, string>,
}
```

## D.5 Loom 与 OpenCode 完整对照

| OpenCode v2 | Loom StreamEvent | Loom SSE | 状态 |
|---|---|---|---|
| `agent.switched` | — | — | ❌ 未实现 |
| `model.switched` | — | — | ❌ 未实现 |
| `moved` | — | — | ❌ 未实现 |
| `prompted` | — | — | ❌ 未实现 |
| `prompt.admitted` | — | — | ❌ 未实现 |
| `context.updated` | — | — | ❌ 未实现 |
| `synthetic` | — | — | ❌ 未实现 |
| `shell.started` | — | — | ❌ 未实现 |
| `shell.ended` | — | — | ❌ 未实现 |
| `retried` | — | — | ❌ 未实现 |
| **`step.started`** | `TurnStart` | `message.part.updated` (step-start) | ⚠️ 缺 agent/model（G3） |
| **`step.ended`** | `TurnFinish` | `message.part.updated` (step-finish) | ✅ token 结构已对齐（G1） |
| **`step.failed`** | `ProviderError` | `session.error` | ✅ |
| **`text.started`** | `TextBlockStart` | `message.part.updated` (text) | ✅ |
| **`text.delta`** | `TextDelta` | `message.part.delta` | ⚠️ 未发增量（G2） |
| **`text.ended`** | `TextBlockEnd` | `message.part.updated` (text, time.end) | ⚠️ 缺完整 text 字段（G5） |
| **`reasoning.started`** | `ReasoningBlockStart` | `message.part.updated` (reasoning) | ✅ |
| **`reasoning.delta`** | `ReasoningDelta` | `message.part.delta` | ⚠️ 未发增量（G2） |
| **`reasoning.ended`** | `ReasoningBlockEnd` | `message.part.updated` (reasoning, time.end) | ⚠️ 缺完整 text 字段（G5） |
| `tool.input.started` | `ToolCall` | `message.part.updated` (tool pending) | ✅ 合并到 called |
| `tool.input.delta` | — | — | ❌ 未实现 |
| `tool.input.ended` | — | — | ❌ 未实现 |
| **`tool.called`** | `ToolCall` | `message.part.updated` (tool pending) | ✅ |
| `tool.progress` | `ToolOutput` | `message.part.updated` (tool running) | ✅ |
| **`tool.success`** | `ToolEnd` (is_error=false) | `message.part.updated` (tool completed) | ✅ |
| **`tool.failed`** | `ToolError` / `ToolEnd` (is_error=true) | `message.part.updated` (tool error) | ✅ |
| `compaction.started` | — | — | ❌ 未实现 |
| `compaction.delta` | — | — | ❌ 未实现 |
| `compaction.ended` | — | — | ❌ 未实现 |
| `revert.staged` | — | — | ❌ 未实现 |
| `revert.cleared` | — | — | ❌ 未实现 |
| `revert.committed` | — | — | ❌ 未实现 |

## D.6 关键差距汇总

**G2（增量事件）**：Loom 当前只发累积 `part.text`，不发 `message.part.delta`。OpenCode 前端如果需要实时增量渲染（避免每次重传完整文本），需补发 delta。

**G3（agent/model 字段）**：`step.started` 携带 `agent` 和 `model`；Loom 当前 `TurnStart` 单元变体，无字段。

**G5（ended 携带全量 text）**：OpenCode `text.ended` / `reasoning.ended` 携带完整 `text` 字段（可重放），Loom `TextBlockEnd` / `ReasoningBlockEnd` 不携带。

**未实现事件**：retried、compaction.*、revert.*、agent.switched、model.switched、shell.*、context.updated、synthetic、prompted、prompt.admitted、moved、tool.input.*（独立事件流）。

> 注：Loom 当前定位是"OpenCode v1 兼容 + step 边界对齐"，不是 v2 全量对齐。`session.next.*` 32 个事件中，Loom 只覆盖 Step/Text/Reasoning/Tool 的核心 13 个（标记 **bold**）。
