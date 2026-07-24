# 协议影响说明

> 返回 [README.md](README.md)
> 核心目标：使 Loom SSE 输出与 OpenCode v2 schema（`session-event.ts`）对齐，确保 OpenChamber 前端能正确渲染 part 序列。

## 11.0 实现状态

| 变更项 | 代码状态 | 验证状态 |
|---|---|---|
| `Messages` → `TextDelta`/`ReasoningDelta` | ✅ 已落地 | ❌ 未做协议级验证 |
| `TextBlockStart`/`TextBlockEnd` | ✅ 已落地 | ❌ 未做协议级验证 |
| `ReasoningBlockStart`/`ReasoningBlockEnd` | ✅ 已落地 | ❌ 未做协议级验证 |
| `TurnStart`/`TurnFinish`（step-start/finish） | ✅ 已落地 | ❌ 未做协议级验证 |
| `Usage` → `TurnFinish.usage` | ✅ 已落地 | ❌ 未做协议级验证 |
| `message.tokens` 删除 | ✅ 已落地 | ❌ 未做协议级验证 |
| `BlockTracker` 适配层 | ✅ 适配层（`BlockTrackerSink`） | ❌ 未做协议级验证 |
| OpenChamber 前端渲染 | — | ❌ 未验证 |

## 11.1 当前 SSE 事件清单

改造前 translator 对每个 `StreamEvent` 发出的 SSE 事件：

| StreamEvent | SSE 事件类型 | payload |
|---|---|---|
| `Messages { chunk }` (追加) | `message.part.updated` | `{ sessionID, part }` — 累积文本的完整 part |
| `Messages { chunk }` (新建) | `message.part.updated` | `{ sessionID, part }` — 新 part（含 `time.start`） |
| `ToolCall` | `message.part.updated` | `{ sessionID, part }` — `type: "tool"`，`status: "pending"` |
| `ToolStart` | `message.part.updated` | `status: "running"` |
| `ToolOutput` | `message.part.updated` | 追加 `output` |
| `ToolEnd` | `message.part.updated` | `status: "completed"` / `"error"`，`time.end` |
| `Usage` | `message.tokens` | `{ sessionID, messageID, input, output }` |

运行结束时额外发送：

| 触发点 | SSE 事件类型 | payload |
|---|---|---|
| `close_open_text_parts` | `message.part.updated` | 为每个未关闭的 text/reasoning part 补盖 `time.end` |
| run 完成（正常） | `message.updated` | `{ sessionID, info: { id, role: "assistant", finish: "stop" } }` |
| run 完成（取消） | `message.updated` | `finish: "cancelled"` |
| run 完成（错误） | `message.updated` + `session.error` | `finish: "error"` + `{ sessionID, error: { name, data: { message } } }` |
| 状态变更 | `session.status` | `{ sessionID, status: { type: "busy" | "idle" } }` |

## 11.2 改造后 SSE 事件清单

### 11.2.1 新增 SSE 事件

| StreamEvent | SSE 事件类型 | payload | 说明 |
|---|---|---|---|
| `TextBlockStart` | `message.part.updated` | `{ sessionID, part: { id, type: "text", text: "", time: { start, created } } }` | 新建空 text part |
| `TextDelta` | `message.part.updated` | `{ sessionID, part }` — 累积文本 | 追加到 `active_text[msg_id]` 对应的 part |
| `TextBlockEnd` | `message.part.updated` | `{ sessionID, part }` — 加盖 `time.end` / `time.completed` | 收尾 text part |
| `ReasoningBlockStart` | `message.part.updated` | `{ sessionID, part: { id, type: "reasoning", text: "", time: { start, created }, metadata } }` | 新建空 reasoning part |
| `ReasoningDelta` | `message.part.updated` | `{ sessionID, part }` — 累积文本 | 按 `reasoning_id` 追加 |
| `ReasoningBlockEnd` | `message.part.updated` | `{ sessionID, part }` — 加盖 `time.end` | 按 `reasoning_id` 收尾 |
| `TurnStart` | `message.part.updated` | `{ sessionID, part: { id, type: "step-start", time: { start, created } } }` | 新建 step-start part |
| `TurnFinish` | `message.part.updated` | `{ sessionID, part: { id, type: "step-finish", reason, tokens: { prompt, completion, total, cached }, time: { start, end, created } } }` | 新建 step-finish part |
| `ToolError` | `message.part.updated` | `{ sessionID, part }` — `status: "error"` | tool 执行失败 |
| `ProviderError` | `session.error` | `{ sessionID, error: { name, data: { message } } }` | provider 返回错误 |
| `Finish` | （无独立 SSE） | — | 信号事件，translator no-op |

### 11.2.2 删除的 SSE 事件

| 当前事件 | 原因 |
|---|---|
| `message.tokens` | `Usage` 变体删除，token 用量折叠进 `TurnFinish` → `step-finish` part 的 `tokens` 字段 |

### 11.2.3 不变的 SSE 事件

| SSE 事件类型 | 说明 |
|---|---|
| `message.part.updated` | 外层结构 `{ sessionID, part, time }` 不变 |
| `message.updated` | run 结束时的 assistant message 收尾不变 |
| `session.status` | busy / idle 不变 |
| `session.error` | 结构不变（`ProviderError` 复用） |
| `session.created` / `session.updated` / `session.deleted` | 与本次改造无关 |

## 11.3 Part 类型变化

### 11.3.1 现有 part 类型（不变）

| `type` | 触发 | 说明 |
|---|---|---|
| `text` | `TextBlockStart` → `TextDelta` ×N → `TextBlockEnd` | LLM 回复文本 |
| `reasoning` | `ReasoningBlockStart` → `ReasoningDelta` ×N → `ReasoningBlockEnd` | LLM 推理过程 |
| `tool` | `ToolCall` → `ToolStart` → `ToolOutput` ×N → `ToolEnd` | 工具调用 |

### 11.3.2 新增 part 类型

| `type` | 触发 | payload |
|---|---|---|
| `step-start` | `TurnStart` | `{ id, type: "step-start", time: { start, created } }` |
| `step-finish` | `TurnFinish` | `{ id, type: "step-finish", reason, tokens: { prompt, completion, total, cached }, time: { start, end, created } }` |

### 11.3.3 part 时间戳语义

| 字段 | v1 | v2 | 设置时机 |
|---|---|---|---|
| `time.start` | ✅ | — | `*BlockStart` / `step-start` 创建时 |
| `time.created` | — | ✅ | 同上 |
| `time.end` | ✅ | — | `*BlockEnd` / `step-finish` 收尾时 |
| `time.completed` | — | ✅ | 同上 |

> 改造前：`time.end` 仅在 run 结束时由 `close_open_text_parts` 统一补盖。
> 改造后：每对 `BlockStart/End` 精确设置各自的 `time.start` / `time.end`。

## 11.4 事件序列对比

### 11.4.1 单回合（reasoning → text → tool）

**改造前：**

```
session.status { busy }
message.part.updated { type: "reasoning", text: "Let me" }
message.part.updated { type: "reasoning", text: "Let me think" }
message.part.updated { type: "text", text: "Running" }
message.part.updated { type: "text", text: "Running ls" }
message.part.updated { type: "tool", status: "pending" }
message.part.updated { type: "tool", status: "running" }
message.part.updated { type: "tool", status: "completed" }
message.tokens { input: 100, output: 50 }
session.status { idle }
```

**改造后：**

```
session.status { busy }
message.part.updated { type: "step-start" }
message.part.updated { type: "reasoning", text: "Let me" }
message.part.updated { type: "reasoning", text: "Let me think" }
message.part.updated { type: "reasoning", time.end ✅ }
message.part.updated { type: "text", text: "Running" }
message.part.updated { type: "text", text: "Running ls" }
message.part.updated { type: "text", time.end ✅ }
message.part.updated { type: "tool", status: "pending" }
message.part.updated { type: "tool", status: "running" }
message.part.updated { type: "tool", status: "completed" }
message.part.updated { type: "step-finish", tokens: { prompt: 100, completion: 50, ... } }
session.status { idle }
```

### 11.4.2 多回合（reasoning → text → tool → reasoning → text）

改造前缺少回合边界和 reasoning 收尾，`message.tokens` 事件散落。

改造后每个 LLM 回合被 `step-start` / `step-finish` 明确包裹，reasoning 在回合内精确收尾，token 用量附在 `step-finish` 上。

## 11.5 客户端适配清单

| # | 客户端改动 | 必需 | 说明 |
|---|---|---|---|
| C1 | 渲染 `step-start` / `step-finish` part | ✅ | 若忽略则仅影响 UI 显示，不破坏功能 |
| C2 | 移除 `message.tokens` 事件监听 | ✅ | 该事件不再发送，token 数据改从 `step-finish` part 的 `tokens` 字段读取 |
| C3 | 移除对 `message.part.updated` payload 格式的假设 | ✅ | part 的 `type` 字段新增 `step-start` / `step-finish` |
| C4 | 渲染 `session.error` | 可选 | ProviderError 触发；现有错误处理可能已覆盖 |
| C5 | 支持 `message.part.delta`（增量事件） | 可选 | 仅 X1（独立增强）实现后才有；不实现则忽略，继续用 `message.part.updated` |

## 11.6 Token 用量字段对照

| 字段 | 改造前（`message.tokens`） | 改造后（`step-finish` part `tokens`） |
|---|---|---|
| prompt | `input` | `tokens.prompt` |
| completion | `output` | `tokens.completion` |
| total | 不发送 | `tokens.total` |
| cached | 不发送 | `tokens.cached` |
