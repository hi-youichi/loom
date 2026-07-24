# Loom SSE 返回结构参考

状态：当前实现的 wire contract。更新日期：2026-07-24。目标 OpenCode schema revision：`b8142c7aa`。

本文描述 Loom 实际通过 SSE 返回的完整 JSON 结构；它不是测试要求清单。字段后缀 `?` 表示该字段在 JSON 中可省略，省略时不得输出为 `null`。除明确写为 live 的四个事件外，所有 `session.next.*` 均为 durable event。

## 1. 三条 SSE stream

| URL | SSE `event:` | data JSON | 包含内容 |
| --- | --- | --- | --- |
| `/global/event` | Axum 默认 `message` | legacy wrapper | legacy `server.*`、`session.*`、`message.*` 事件；有连接/heartbeat |
| `/api/event` | `message` | v2 flat envelope | 所有 v2 durable 和 live 事件 |
| `/api/session/:sessionID/event?after=<seq>` | `message` | v2 flat envelope | 仅该 session durable event 的 replay 和后续 durable event |

v2 的一帧例如：

```text
event: message
data: {"id":"evt_v2_1","type":"session.next.text.ended","durable":{"aggregateID":"sess_1","seq":8,"version":1},"data":{"timestamp":1760000000000,"sessionID":"sess_1","assistantMessageID":"msg_a","textID":"part_t","text":"done"}}

```

`after` 是已消费的 durable `seq`：只返回 `seq > after`。`/api/event` 不带历史；`/api/session/.../event` 不返回 live event。无效的非数字 `after` 返回 HTTP 400。

## 2. 外层 envelope

### 2.1 v2 flat envelope

```ts
type V2Event = {
  id: string                 // evt_v2_<monotonic id>
  metadata?: Record<string, unknown>
  type: SessionNextType
  durable?: {
    aggregateID: string      // 等于 data.sessionID
    seq: number              // 每个 session 从 1 开始连续递增
    version: 1 | 2
  }
  location?: { directory: string; workspaceID?: string }
  data: SessionNextData
}
```

durable 行必须有 `durable`。`step.ended` 和 `step.failed` 使用 version 2；其余 durable 类型使用 version 1。下列 live-only 类型没有 `durable`：`text.delta`、`reasoning.delta`、`tool.input.delta`、`compaction.delta`。

### 2.2 legacy wrapper（只用于 `/global/event`）

```ts
type LegacyEvent = {
  directory: string
  payload: { id: string; type: string; properties: unknown }
}
```

legacy stream 可含 `server.connected` 和 `server.heartbeat`；它们不是 v2 `session.next.*` 类型，也不会出现在两个 `/api/*/event` v2 stream。

## 3. 共享嵌套类型

每种 `data` 都包含：

```ts
type Base = { timestamp: number; sessionID: string }
type UnknownError = { type: "unknown"; message: string }
type ModelRef = { id: string; providerID: string; variant?: string }
type LocationRef = { directory: string; workspaceID?: string }
type Tokens = { input: number; output: number; reasoning: number; cache: { read: number; write: number } }
type ProviderMetadata = Record<string, Record<string, unknown>>
type Provider = { executed: boolean; metadata?: ProviderMetadata }
type ToolContent =
  | { type: "text"; text: string }
  | { type: "file"; uri: string; mime: string; name?: string }
```

```ts
type Prompt = {
  text: string
  files?: { uri: string; mime: string; name?: string; description?: string; source?: { start: number; end: number; text: string } }[]
  agents?: { name: string; source?: { start: number; end: number; text: string } }[]
}
type Delivery = "steer" | "queue"
```

`snapshot` 表示消息时的工作区快照；`files` 是相对路径数组。所有 ID 都是字符串，且同一 session 内 text、reasoning、tool 的生命周期 ID 必须稳定复用。

## 4. 全部 32 个 `session.next.*` data 结构

以下每个定义均隐式交叉 `Base`。`A` 为 `assistantMessageID: string`。

### 4.1 通用、prompt、shell 与 retry（10）

```ts
type AgentSwitched = Base & { messageID: string; agent: string }
type ModelSwitched = Base & { messageID: string; model: ModelRef }
type Moved = Base & { location: LocationRef; subdirectory?: string }
type Prompted = Base & { messageID: string; prompt: Prompt; delivery: Delivery }
type PromptAdmitted = Base & { messageID: string; prompt: Prompt; delivery: Delivery }
type ContextUpdated = Base & { messageID: string; text: string }
type Synthetic = Base & { messageID: string; text: string }
type ShellStarted = Base & { messageID: string; callID: string; command: string }
type ShellEnded = Base & { callID: string; output: string }
type Retried = Base & { attempt: number; error: { message: string; isRetryable: boolean; statusCode?: number; responseHeaders?: Record<string, string>; responseBody?: string; metadata?: Record<string, string> } }
```

对应 type 依次为 `session.next.agent.switched`、`model.switched`、`moved`、`prompted`、`prompt.admitted`、`context.updated`、`synthetic`、`shell.started`、`shell.ended`、`retried`。`prompted` 表示接收请求，`prompt.admitted` 表示 user message 已成功持久化；不得只因请求到达就发 admitted。

### 4.2 step（3）

```ts
type StepStarted = Base & { assistantMessageID: string; agent: string; model: ModelRef; snapshot?: string }
type StepEnded = Base & { assistantMessageID: string; finish: string; cost: number; tokens: Tokens; snapshot?: string; files?: string[] }
type StepFailed = Base & { assistantMessageID: string; error: UnknownError }
```

对应 `session.next.step.started`、`step.ended`、`step.failed`。后两个是 version 2 durable settlement，且对同一 `assistantMessageID` 互斥。

### 4.3 text（3）

```ts
type TextStarted = Base & { assistantMessageID: string; textID: string }
type TextDelta = Base & { assistantMessageID: string; textID: string; delta: string }
type TextEnded = Base & { assistantMessageID: string; textID: string; text: string }
```

对应 `session.next.text.started`、`text.delta`、`text.ended`。`delta` live-only；重连端通过 started/ended 而不是历史 delta 恢复完整正文。

### 4.4 reasoning（3）

```ts
type ReasoningStarted = Base & { assistantMessageID: string; reasoningID: string; providerMetadata?: ProviderMetadata }
type ReasoningDelta = Base & { assistantMessageID: string; reasoningID: string; delta: string }
type ReasoningEnded = Base & { assistantMessageID: string; reasoningID: string; text: string; providerMetadata?: ProviderMetadata }
```

对应 `session.next.reasoning.started`、`reasoning.delta`、`reasoning.ended`。`reasoning.delta` live-only；metadata 不可映射时应省略。

### 4.5 tool（7）

```ts
type ToolInputStarted = Base & { assistantMessageID: string; callID: string; name: string }
type ToolInputDelta = Base & { assistantMessageID: string; callID: string; delta: string }
type ToolInputEnded = Base & { assistantMessageID: string; callID: string; text: string }
type ToolCalled = Base & { assistantMessageID: string; callID: string; tool: string; input: Record<string, unknown>; provider: Provider }
type ToolProgress = Base & { assistantMessageID: string; callID: string; structured: Record<string, unknown>; content: ToolContent[] }
type ToolSuccess = Base & { assistantMessageID: string; callID: string; structured: Record<string, unknown>; content: ToolContent[]; outputPaths?: string[]; result?: unknown; provider: Provider }
type ToolFailed = Base & { assistantMessageID: string; callID: string; error: UnknownError; result?: unknown; provider: Provider }
```

对应 `session.next.tool.input.started`、`input.delta`、`input.ended`、`called`、`progress`、`success`、`failed`。`input.delta` live-only；`input.ended.text` 是参数的规范 JSON 文本，`called.input` 是同一参数的 object。每个 `callID` 最多有一个 terminal：`success` 或 `failed`。

### 4.6 compaction（3）

```ts
type CompactionStarted = Base & { messageID: string; reason: "auto" | "manual" }
type CompactionDelta = Base & { messageID: string; text: string }
type CompactionEnded = Base & { messageID: string; reason: "auto" | "manual"; text: string; recent: string }
```

对应 `session.next.compaction.started`、`compaction.delta`、`compaction.ended`。`compaction.delta` live-only；`ended.text` 是最终摘要，`recent` 是保留的最近上下文条目。

### 4.7 revert（3）

```ts
type FileDiff = { path: string; status: "added" | "modified" | "deleted"; additions: number; deletions: number; patch: string }
type RevertState = { messageID: string; partID?: string; snapshot?: string; diff?: string; files?: FileDiff[] }
type RevertStaged = Base & { revert: RevertState }
type RevertCleared = Base
type RevertCommitted = Base & { messageID: string }
```

对应 `session.next.revert.staged`、`revert.cleared`、`revert.committed`。`staged` 保存可观察的 staged state；`cleared` 表示该 state 已删除；`committed.messageID` 是实际提交/回退到的消息。

## 5. 跨类型顺序与一致性

1. durable `seq` 以 session 为单位严格递增，`durable.aggregateID === data.sessionID`。
2. text、reasoning 与 tool 都必须先发 start，才可发 delta/end；终态后不得继续发 delta。
3. live delta 不写入 JSONL durable log，不能通过 session replay 得到。
4. 可选字段完全缺席，而非 JSON `null`；消费者必须接受未出现的 metadata、snapshot、files、result 等字段。
5. `/api/event` 允许不同 session 的事件交织；session stream 只允许目标 session 的 durable sequence。

## 6. 与实现的对应

外层序列化和 durable log 位于 `apps/server/src/v2_event.rs`；三个 SSE route 位于 `apps/server/src/sse.rs`；prompt、shell、retry、compaction、revert 的 publisher 位于 `apps/server/src/handlers/session.rs`；provider/agent stream 到 step、text、reasoning、tool event 的转换位于 `apps/server/src/translator.rs`。逐类型来源和验收测试入口见 [类型矩阵](LOOM-V2-SSE-TYPE-MATRIX.md) 与 [集成测试方案](SERVER-SSE-INTEGRATION-TEST-PLAN.md)。
