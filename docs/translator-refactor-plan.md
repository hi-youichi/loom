# Translator 重构：对齐 OpenCode 的 Part 生命周期管理

## 背景

Loom translator (`apps/server/src/translator.rs`) 当前的 text/reasoning part 追加逻辑存在缺陷：

- **当前方案**：每次收到 chunk 时，按 `part_type` 搜索 parts 列表的最后一个元素进行匹配追加
- **问题**：无法正确处理 `reasoning → text → tool → reasoning → text` 的交替序列

OpenCode (`packages/opencode/src/session/processor.ts`) 使用**显式状态引用 + 生命周期事件**模型，
天然解决了这些问题。本文档逐逻辑对比两者，并给出 Loom 的修改方案。

---

> **核实说明**（2025-08-19）：本文档中引用的 `processor.ts` 行号基于 2025-07 代码快照，
> 文件后续已重构（函数内联到 `Effect.gen` 闭包中），行号已偏移。**逻辑描述经核实仍与当前
> `dev` 分支源码一致**。`event-reducer.ts` 在当前源码中不存在（见 §2.7 修正）。

## 一、OpenCode 的核心架构

### 1.1 ProcessorContext 状态

```typescript
// packages/opencode/src/session/processor.ts:112-126
interface ProcessorContext {
  // ...
  currentText: SessionV1.TextPart | undefined       // 当前活跃的 text part（单例）
  reasoningMap: Record<string, SessionV1.ReasoningPart>  // 活跃 reasoning parts（按 stream ID 索引）
}
```

- `currentText`：同一时刻最多只有 **一个** 活跃 text part
- `reasoningMap`：按 LLM stream 返回的 reasoning ID 索引，支持并发 reasoning blocks（Anthropic adaptive thinking）

### 1.2 每次 LLM Stream 运行前重置

```typescript
// packages/opencode/src/session/processor.ts:498-499
// process() 开头：
ctx.currentText = undefined
ctx.reasoningMap = {}
```

- **每个 step（LLM 调用回合）开始时，活跃 part 状态被清空**
- 上一个 step 的 part 已经在 `text-end` / `reasoning-end` 中收尾，这里只是防御性清除

### 1.3 事件类型映射

LLM SDK 返回的流式事件：

| LLM SDK 事件 | OpenCode 处理 | Part 动作 |
|---|---|---|
| `text-start` | 创建 `ctx.currentText` | `updatePart`（完整写入） |
| `text-delta` | `ctx.currentText.text += delta` | `updatePartDelta`（增量） |
| `text-end` | 设置 `time.end`，`ctx.currentText = undefined` | `updatePart`（完整写入） |
| `reasoning-start` | 创建 `reasoningMap[id]` | `updatePart`（完整写入） |
| `reasoning-delta` | `reasoningMap[id].text += delta` | `updatePartDelta`（增量） |
| `reasoning-end` | 设置 `time.end`，从 map 删除 | `updatePart`（完整写入） |
| `tool-input-start` | 创建 pending tool part（含 summary guard） | `updatePart`（完整写入） |
| `tool-input-delta` | `ensureToolCall`（幂等，已存在则跳过） | 无（或已创建） |
| `tool-input-end` | `ensureToolCall`（幂等） | 无 |
| `tool-call` | 更新 tool part → running | `updatePart`（完整写入） |
| `tool-result` | 更新 tool part → completed（含图片归一化） | `updatePart`（完整写入） |
| `tool-error` | 更新 tool part → error（独立于 tool-result） | `updatePart`（完整写入） |
| `provider-error` | throw Error → 进入 retry/halt 管线 | 无（异常路径） |
| `step-start` | 创建 `step-start` part | `updatePart`（完整写入） |
| `step-finish` | 收尾 reasoning + patch part + compaction check | `finishReasoning` + `updatePart` |
| `finish` | no-op（显式忽略） | 无 |

---

## 二、逐逻辑对比与修改方案

### 2.1 Text Part 生命周期

#### OpenCode 处理

```typescript
// text-start：创建新 part
// processor.ts:493-501
case "text-start":
  ctx.currentText = {
    id: PartID.ascending(),
    messageID: ctx.assistantMessage.id,
    sessionID: ctx.assistantMessage.sessionID,
    type: "text",
    text: "",
    time: { start: Date.now() },
    metadata: value.providerMetadata,
  }
  yield* session.updatePart(ctx.currentText)
  return

// text-delta：追加到 currentText
// processor.ts:503-512
case "text-delta":
  if (!ctx.currentText) return
  ctx.currentText.text += value.text
  if (value.providerMetadata) ctx.currentText.metadata = value.providerMetadata
  yield* session.updatePartDelta({
    sessionID: ctx.currentText.sessionID,
    messageID: ctx.currentText.messageID,
    partID: ctx.currentText.id,
    field: "text",
    delta: value.text,
  })
  return

// text-end：设置 time.end，清空 currentText
// processor.ts:514-530
case "text-end":
  if (!ctx.currentText) return
  ctx.currentText.text = ctx.currentText.text  // reactivity trigger
  ctx.currentText.text = (yield* plugin.trigger(...)).text
  {
    const end = Date.now()
    ctx.currentText.time = { start: ctx.currentText.time?.start ?? end, end }
  }
  if (value.providerMetadata) ctx.currentText.metadata = value.providerMetadata
  yield* session.updatePart(ctx.currentText)
  ctx.currentText = undefined  // ← 关键：清空活跃引用
  return
```

**关键设计**：
- `text-start` **总是** 创建新 part，不检查是否有旧 part
- `text-delta` 只在有活跃 `ctx.currentText` 时追加，否则丢弃
- `text-end` 清空 `ctx.currentText`，下次 `text-start` 必然创建新 part

#### Loom 当前代码（有问题）

```rust
// translator.rs:429-441 — 当前逻辑
{
    let mut parts = state.parts.write();
    if let Some(list) = parts.get_mut(assistant_msg_id) {
        if let Some(last) = list.last_mut() {
            if last.part_type == part_type {  // 只看最后一个是否同类型
                // 追加，return
            }
        }
    }
}
// 否则创建新 part
```

**问题**：
- 无显式状态追踪，靠"最后一个 part 类型是否匹配"推断是否应该追加
- tool → text 场景能正确创建新 part（last 是 tool）
- 但如果 LLM 连续输出 `text → reasoning → text`（中间无 tool），第二个 text 会追加到第一个 text
  （因为 last 是 reasoning，不匹配，会创建新 text — 这恰好是对的）
- 实际上当前 `last_mut` 方案在大部分场景能工作，但缺乏显式边界管理

#### Loom 修改方案

```rust
// state.rs — 新增 ActivePart 结构和 SharedState 字段

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

/// 当前正在流式追加的 text 或 reasoning part。
/// 同一时刻最多只有一个活跃 part（等价于 OpenCode 的 currentText / 单个 reasoningMap entry）。
#[derive(Clone, Debug)]
pub struct ActivePart {
    /// "text" 或 "reasoning"
    pub part_type: &'static str,
    /// Part 的唯一 ID（prt_<uuid>）
    pub part_id: String,
    /// 累积的文本内容
    pub text: String,
    /// 创建时间戳（毫秒）
    pub start_ms: i64,
}

// SharedState 新增字段：
pub struct SharedState {
    // ...existing fields...

    /// 按 assistant_msg_id 索引的当前活跃流式 part。
    /// 等价于 OpenCode 的 ctx.currentText + ctx.reasoningMap 的"当前唯一活跃"概念。
    pub active: Arc<RwLock<HashMap<String, ActivePart>>>,
}
```

`translate_chunk` 改为：

```rust
// translator.rs — 新版 translate_chunk

fn translate_chunk(
    chunk: &MessageChunk,
    session_id: &str,
    assistant_msg_id: &str,
    state: &SharedState,
) {
    let want_type = if chunk.is_thinking() { "reasoning" } else { "text" };

    // ── 步骤 1：检查是否需要切换 part ──────────────────────
    // OpenCode 通过显式的 text-start / reasoning-start 事件触发新建。
    // Loom 没有 start 事件，需要推断：当前活跃 part 类型 ≠ 期望类型 → 边界切换。
    let need_new = {
        let active = state.active.read();
        match active.get(assistant_msg_id) {
            None => true,                                // 无活跃 part → 新建
            Some(a) => a.part_type != want_type,         // 类型不同 → 先收尾，再新建
        }
    };

    if need_new {
        // ── 步骤 2：收尾旧 part（等价于 OpenCode text-end / reasoning-end）──
        finalize_active_part(state, session_id, assistant_msg_id);

        // ── 步骤 3：创建新 part（等价于 OpenCode text-start / reasoning-start）──
        let now_ms = chrono::Utc::now().timestamp_millis();
        let part_id = new_part_id();

        push_part(
            state,
            assistant_msg_id,
            session_id,
            want_type,
            json!({
                "id": part_id,
                "type": want_type,
                "text": chunk.content,
                "time": { "start": now_ms, "created": now_ms },
            }),
        );

        // 记录为活跃
        state.active.write().insert(
            assistant_msg_id.to_string(),
            ActivePart {
                part_type: want_type,
                part_id: part_id,
                text: chunk.content.to_string(),
                start_ms: now_ms,
            },
        );
    } else {
        // ── 步骤 4：追加到活跃 part（等价于 OpenCode text-delta / reasoning-delta）──
        let mut active = state.active.write();
        let a = active.get_mut(assistant_msg_id).expect("checked above");
        a.text.push_str(&chunk.content);

        // 同步到 state.parts 并 emit
        let payload = {
            let mut parts = state.parts.write();
            if let Some(list) = parts.get_mut(assistant_msg_id) {
                if let Some(p) = list.iter_mut().find(|p| p.id == a.part_id) {
                    p.data["text"] = json!(a.text);
                    Some(p.data.clone())
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(payload) = payload {
            emit(
                state,
                "message.part.updated",
                json!({
                    "sessionID": session_id,
                    "part": payload,
                    "time": chrono::Utc::now().timestamp_millis(),
                }),
            );
        }
    }
}
```

---

### 2.2 Reasoning Part 生命周期

#### OpenCode 处理

```typescript
// reasoning-start：创建新 part，存入 reasoningMap
// processor.ts:177-188
case "reasoning-start":
  if (value.id in ctx.reasoningMap) return  // 幂等：已存在则跳过
  ctx.reasoningMap[value.id] = {
    id: PartID.ascending(),
    messageID: ctx.assistantMessage.id,
    sessionID: ctx.assistantMessage.sessionID,
    type: "reasoning",
    text: "",
    time: { start: Date.now() },
    metadata: value.providerMetadata,
  }
  yield* session.updatePart(ctx.reasoningMap[value.id])
  return

// reasoning-delta：追加文本到 map 中的 part
// processor.ts:190-201
case "reasoning-delta":
  if (!(value.id in ctx.reasoningMap)) return  // 孤儿 delta 丢弃
  ctx.reasoningMap[value.id].text += value.text
  if (value.providerMetadata) ctx.reasoningMap[value.id].metadata = value.providerMetadata
  yield* session.updatePartDelta({
    sessionID: ctx.reasoningMap[value.id].sessionID,
    messageID: ctx.reasoningMap[value.id].messageID,
    partID: ctx.reasoningMap[value.id].id,
    field: "text",
    delta: value.text,
  })
  return

// reasoning-end：收尾并从 map 删除
// processor.ts:203-208
case "reasoning-end":
  if (value.providerMetadata && value.id in ctx.reasoningMap) {
    ctx.reasoningMap[value.id].metadata = value.providerMetadata
  }
  yield* finishReasoning(value.id)  // 设置 time.end，updatePart，delete from map
  return

// finishReasoning 实现：
// processor.ts:148-155
const finishReasoning = Effect.fn("SessionProcessor.finishReasoning")(function* (reasoningID: string) {
  if (!(reasoningID in ctx.reasoningMap)) return
  ctx.reasoningMap[reasoningID].text = ctx.reasoningMap[reasoningID].text  // reactivity trigger
  ctx.reasoningMap[reasoningID].time = { ...ctx.reasoningMap[reasoningID].time, end: Date.now() }
  yield* session.updatePart(ctx.reasoningMap[reasoningID])
  delete ctx.reasoningMap[reasoningID]  // ← 关键：从 map 删除
})
```

**关键设计**：
- `reasoningMap` 按 LLM 返回的 reasoning stream ID 索引
- `reasoning-start` 总是创建新 part，幂等检查防重复
- `reasoning-delta` 只更新 map 中已有的 part
- `reasoning-end` 收尾（`time.end`）并从 map 删除
- `step-finish` 会批量收尾所有 `reasoningMap` 中剩余的 reasoning（`processor.ts:417`）

#### Loom 当前代码

```rust
// translator.rs:416-418 — reasoning 和 text 共用同一段逻辑
let part_type = if chunk.is_thinking() {
    "reasoning"
} else {
    "text"
};
// 然后走与 text 完全相同的 last_mut 匹配路径
```

Loom 不区分 reasoning 和 text 的处理路径，共用 `translate_chunk`。

#### Loom 修改方案

Reasoning 与 text 共用 `translate_chunk` 中的活跃 part 管理逻辑（上一节已实现），
因为 Loom 的 `MessageChunk` 只有 `is_thinking()` 区分，没有独立的 stream ID。

**Loom MessageChunk vs OpenCode LLMEvent**：

| OpenCode LLMEvent | Loom MessageChunk | 区别 |
|---|---|---|
| `text-start` (有 ID) | `MessageChunk { kind: Message, content }` | Loom 无 start/delta/end 区分 |
| `text-delta` | 同上 | 每个 chunk 都是完整增量 |
| `text-end` | 无对应事件 | Loom 靠 run 结束推断 |
| `reasoning-start` (有 ID) | `MessageChunk { kind: Thinking, content }` | Loom 无独立 reasoning ID |
| `reasoning-delta` | 同上 | 每个 chunk 都是完整增量 |
| `reasoning-end` | 无对应事件 | Loom 靠 run 结束推断 |

由于 Loom 的 `MessageChunk` 没有 `start`/`delta`/`end` 三段式，
`translate_chunk` 需要自行推断 part 边界——这就是 `ActivePart` 状态的核心作用。

**简化模型**：Loom 同一时刻只可能有一个活跃 part（text 或 reasoning），
等价于 OpenCode 的 `ctx.currentText` + 单一活跃 reasoning entry 的合并。
当 `MessageChunk` 的 `is_thinking()` 状态翻转时，自动收尾旧 part 并创建新 part。

不需要单独的 `reasoningMap`，因为 Loom 的 stream 不提供并发 reasoning blocks。

---

### 2.3 Tool Part 与 Text/Reasoning 的边界

#### OpenCode 处理

```typescript
// tool-input-start：创建 pending tool part（含 summary guard）
case "tool-input-start":
  if (ctx.assistantMessage.summary) {
    throw new Error(`Tool call not allowed while generating summary: ${value.name}`)
  }
  yield* ensureToolCall(value)  // 创建 tool part，不触及 currentText
  return

// tool-input-delta / tool-input-end：幂等调用 ensureToolCall
case "tool-input-delta":
  yield* ensureToolCall(value)  // 已存在则 no-op
  return
case "tool-input-end":
  yield* ensureToolCall(value)
  return

// tool-call：更新 tool part → running（含 summary guard）
case "tool-call":
  if (ctx.assistantMessage.summary) {
    throw new Error(`Tool call not allowed while generating summary: ${value.name}`)
  }
  yield* ensureToolCall(value)
  const input = isRecord(value.input) ? value.input : { value: value.input }
  yield* updateToolCall(value.id, (match) => ({
    ...match,
    tool: value.name,
    state: match.state.status === "running"
      ? { ...match.state, input }
      : { status: "running", input, time: { start: Date.now() } },
    metadata: match.metadata?.providerExecuted
      ? { ...value.providerMetadata, providerExecuted: true }
      : value.providerMetadata,
  }))
  // 注意：tool-call 内还包含 doom loop 检测（安全特性，不在本文档范围内）
  return

// tool-error：独立于 tool-result 的错误路径
case "tool-error":
  yield* failToolCall(value.id, value.error ?? new Error(value.message))
  return

// provider-error：直接 throw，进入 retry/halt 管线（见 §2.10）
case "provider-error":
  throw new Error(value.message)

// finish：显式 no-op
case "finish":
  return
```

OpenCode 不在 tool 事件中收尾 text/reasoning——它依赖 `text-end` 事件已经先行到达。
LLM SDK 的事件顺序保证了 `text-end` → `tool-input-start` 的时序。

#### Loom 修改方案

Loom 的 stream 事件顺序同样是 `Messages(text) → ToolCall → Messages(text) ...`，
但没有显式的 `text-end` 事件。因此 **tool 事件必须触发收尾**：

```rust
// translator.rs — translate_stream_event 中

StreamEvent::ToolCall { call_id, name, arguments } => {
    // ── 收尾当前活跃的 text/reasoning part ──
    // OpenCode 依赖 text-end 事件先行到达，Loom 没有该事件，
    // 在 tool 边界显式收尾。
    finalize_active_part(state, session_id, assistant_msg_id);

    create_or_update_tool_part(
        state,
        assistant_msg_id,
        session_id,
        call_id.as_deref(),
        name,
        ToolTransition::Create { input: arguments.clone() },
    );
}

StreamEvent::ToolStart { call_id, name } => {
    // tool 已在 ToolCall 收尾 text/reasoning，这里不需要再收尾
    create_or_update_tool_part(
        state, assistant_msg_id, session_id,
        call_id.as_deref(), name,
        ToolTransition::Start,
    );
}

StreamEvent::ToolOutput { call_id, name, content } => {
    create_or_update_tool_part(
        state, assistant_msg_id, session_id,
        call_id.as_deref(), name,
        ToolTransition::AppendOutput(content.clone()),
    );
}

StreamEvent::ToolEnd { call_id, name, result, is_error, raw_result } => {
    create_or_update_tool_part(
        state, assistant_msg_id, session_id,
        call_id.as_deref(), name,
        ToolTransition::Finish {
            output: raw_result.clone().unwrap_or_else(|| result.clone()),
            is_error: *is_error,
        },
    );
}
```

---

### 2.4 Run 结束时的收尾（cleanup）

#### OpenCode 处理

```typescript
const cleanup = Effect.fn("SessionProcessor.cleanup")(function* () {
  // 1. 快照/patch：如果存在未提交的 snapshot，生成 patch part
  if (ctx.snapshot) {
    const patch = yield* snapshot.patch(ctx.snapshot)
    if (patch.files.length) {
      yield* session.updatePart({
        id: PartID.ascending(), messageID, sessionID,
        type: "patch", hash: patch.hash, files: patch.files,
      })
    }
    ctx.snapshot = undefined
  }

  // 2. 收尾活跃 text part
  if (ctx.currentText) {
    const end = Date.now()
    ctx.currentText.time = { start: ctx.currentText.time?.start ?? end, end }
    yield* session.updatePart(ctx.currentText)
    ctx.currentText = undefined
  }

  // 3. 收尾所有活跃 reasoning parts
  for (const part of Object.values(ctx.reasoningMap)) {
    const end = Date.now()
    yield* session.updatePart({
      ...part,
      time: { start: part.time.start ?? end, end },
    })
  }
  ctx.reasoningMap = {}

  // 4. 等待在途 tool 自然完成（最多 250ms 宽限期）
  yield* Effect.forEach(
    Object.values(ctx.toolcalls),
    (call) => Deferred.await(call.done).pipe(Effect.timeout("250 millis"), Effect.ignore),
    { concurrency: "unbounded" },
  )

  // 5. 标记所有仍未完成的 tool parts → error + interrupted
  for (const toolCallID of Object.keys(ctx.toolcalls)) {
    const match = yield* readToolCall(toolCallID)
    if (!match) continue
    const part = match.part
    const end = Date.now()
    const metadata = "metadata" in part.state && isRecord(part.state.metadata)
      ? part.state.metadata : {}
    yield* session.updatePart({
      ...part,
      state: {
        ...part.state,
        status: "error",
        error: "Tool execution aborted",
        metadata: { ...metadata, interrupted: true },  // ← 标记中断
        time: { start: "time" in part.state ? part.state.time.start : end, end },
      },
    })
  }
  ctx.toolcalls = {}

  // 6. 标记 message completed
  ctx.assistantMessage.time.completed = Date.now()
  yield* session.updateMessage(ctx.assistantMessage)
})
```

**OpenCode cleanup 的执行时机与管线**（见 §2.10 详述）：
- `Effect.ensuring(cleanup())` — 在 `process()` 的 finally 块中执行
- 无论正常结束、异常、中断，cleanup 都会执行
- cleanup 中的 tool 250ms 宽限期允许在途工具（如 `execute` 的子调用）自然完成后再标记为 aborted

#### Loom 当前代码

```rust
// translator.rs:353-420 — close_open_text_parts()
// 遍历 parts 列表，为所有没有 time.end 的 text/reasoning part 补上
```

#### Loom 修改方案

用 `finalize_active_part` 替代 `close_open_text_parts`：

```rust
// translator.rs — 新版收尾函数

/// 收尾当前活跃的 text/reasoning part。
/// 等价于 OpenCode 的 text-end + finishReasoning。
///
/// 调用时机：
/// 1. chunk 类型切换（text ↔ reasoning）
/// 2. ToolCall 事件到达
/// 3. Run 结束（替代旧版 close_open_text_parts 的 text/reasoning 部分）
pub fn finalize_active_part(
    state: &SharedState,
    session_id: &str,
    assistant_msg_id: &str,
) {
    // 取出并删除活跃引用
    let ended = state.active.write().remove(assistant_msg_id);
    let Some(a) = ended else {
        return;  // 无活跃 part，无需收尾
    };

    let now_ms = chrono::Utc::now().timestamp_millis();

    // 更新 state.parts 中对应 part 的 time.end
    let payload = {
        let mut parts = state.parts.write();
        if let Some(list) = parts.get_mut(assistant_msg_id) {
            if let Some(p) = list.iter_mut().find(|p| p.id == a.part_id) {
                // 设置 time.end + time.completed
                if let Some(t) = p.data.get_mut("time").and_then(|v| v.as_object_mut()) {
                    t.insert("end".into(), json!(now_ms));
                    t.insert("completed".into(), json!(now_ms));
                }
                Some(p.data.clone())
            } else {
                None
            }
        } else {
            None
        }
    };

    // emit 收尾事件
    if let Some(payload) = payload {
        emit(
            state,
            "message.part.updated",
            json!({
                "sessionID": session_id,
                "part": payload,
                "time": now_ms,
            }),
        );
    }
}
```

Session handler 中的调用变更：

```rust
// handlers/session.rs — run_prompt / run_shell 结束处
// 旧：
close_open_text_parts(&state_bg, &sid, &assistant_message_id, ended_at_ms);

// 新：
finalize_active_part(&state_bg, &sid, &assistant_message_id);
// 同时清理可能遗留的 active 引用（防御性）
state_bg.active.write().remove(&assistant_message_id);
```

**OpenCode cleanup 额外职责（Loom 待考虑对齐）**：

| # | OpenCode cleanup 行为 | Loom 当前 | 建议 |
|---|---|---|---|
| A | snapshot patch part 生成 | 无 | Loom 无 snapshot 机制，暂不需要 |
| B | tool 250ms 宽限期等待在途工具完成 | 直接标记 aborted | **应对齐**：避免在途工具被误标为 error |
| C | aborted tool 标记 `interrupted: true` | 无此元数据 | **应对齐**：前端可区分中断 vs 真实错误 |
| D | tool error 文案为 `"Tool execution aborted"` | 可能不同 | 统一文案以便前端处理 |

---

### 2.5 Run 开始时的状态重置

#### OpenCode 处理

```typescript
// processor.ts:498-499 — process() 函数开头
ctx.currentText = undefined
ctx.reasoningMap = {}
```

每次调用 `process()`（等价于 Loom 的一次 `run_agent`）时，
先清空活跃 part 状态。这保证了：
- 上一次 run 的残留状态不会泄漏到新 run
- 即使 `text-end` 事件丢失（异常 / 中断），新 run 也不会追加到旧 part

#### Loom 修改方案

```rust
// agent_runner.rs — run_agent() 开头

pub async fn run_agent(
    state: SharedState,
    session_id: String,
    message_id: String,
    workdir: PathBuf,
    prompt: String,
    model: Option<String>,
    agent_name: Option<String>,
) {
    // ── 重置活跃 part 状态 ──
    // 等价于 OpenCode processor.ts:498-499 的 ctx.currentText = undefined
    state.active.write().remove(&message_id);

    // ...rest of run_agent...
}
```

---

### 2.6 Tool Part 生命周期

#### OpenCode 处理

```typescript
// ensureToolCall：创建或获取 tool part
// processor.ts:163-186
const ensureToolCall = Effect.fn("SessionProcessor.ensureToolCall")(function* (input) {
  const existing = yield* readToolCall(input.id)
  if (existing) return existing

  // 创建新 tool part
  const part = yield* session.updatePart({
    id: PartID.ascending(),
    messageID: ctx.assistantMessage.id,
    sessionID: ctx.assistantMessage.sessionID,
    type: "tool",
    tool: input.name,
    callID: input.id,
    state: { status: "pending", input: {}, raw: "" },
    metadata: input.providerExecuted ? { providerExecuted: true } : undefined,
  })
  ctx.toolcalls[input.id] = {
    done: yield* Deferred.make<void>(),
    partID: part.id,
    messageID: part.messageID,
    sessionID: part.sessionID,
  }
  return { call: ctx.toolcalls[input.id], part }
})

// tool-call 事件：pending → running
// processor.ts:254-285
case "tool-call":
  yield* ensureToolCall(value)
  yield* updateToolCall(value.id, (match) => ({
    ...match,
    tool: value.name,
    state: match.state.status === "running"
      ? { ...match.state, input }
      : { status: "running", input, time: { start: Date.now() } },
  }))
  return

// tool-result 事件：running → completed
// processor.ts:330-365
case "tool-result": {
  const toolCall = yield* readToolCall(value.id)
  if (!toolCall && value.result.type === "error") return
  if (value.result.type === "error") {
    yield* failToolCall(value.id, value.result.value)
    return
  }
  yield* completeToolCall(value.id, output)
  return
}

// completeToolCall：running → completed
// processor.ts:124-142
const completeToolCall = Effect.fn("SessionProcessor.completeToolCall")(function* (
  toolCallID, output,
) {
  const match = yield* readToolCall(toolCallID)
  if (!match || match.part.state.status !== "running") return
  yield* session.updatePart({
    ...match.part,
    state: {
      status: "completed",
      input: match.part.state.input,
      output: output.output,
      metadata: output.metadata,
      title: output.title,
      time: { start: match.part.state.time.start, end: Date.now() },
      attachments: output.attachments,
    },
  })
  yield* settleToolCall(toolCallID)
})

// failToolCall：running → error
// processor.ts:144-160
const failToolCall = Effect.fn("SessionProcessor.failToolCall")(function* (toolCallID, error) {
  const match = yield* readToolCall(toolCallID)
  if (!match || match.part.state.status !== "running") return false
  yield* session.updatePart({
    ...match.part,
    state: {
      status: "error",
      input: match.part.state.input,
      error: errorMessage(error),
      metadata: match.part.state.metadata,
      time: { start: match.part.state.time.start, end: Date.now() },
    },
  })
  // 权限拒绝/用户取消 → 设置 blocked，控制 agent loop 是否终止
  if (error instanceof PermissionV1.RejectedError || error instanceof Question.RejectedError) {
    ctx.blocked = ctx.shouldBreak
  }
  yield* settleToolCall(toolCallID)
  return true
})

// tool-error 事件：独立于 tool-result 的 SDK 错误路径
case "tool-error":
  yield* failToolCall(value.id, value.error ?? new Error(value.message))
  return

// tool-result 事件中的图片归一化：
// 对 image/* 附件做 image.normalize()，过大时统计 omitted 数量并追加到 output 文本
const rawOutput = toolResultOutput(value)  // 提取 title/metadata/output/attachments
const normalized = yield* Effect.forEach(rawOutput.attachments ?? [], (attachment) =>
  attachment.mime.startsWith("image/")
    ? image.normalize(attachment).pipe(
        Effect.catchIf(error => error instanceof Image.ResizerUnavailableError, () => Effect.succeed(attachment)),
        Effect.exit,
      )
    : Effect.succeed(Exit.succeed<SessionV1.FilePart>(attachment)),
)
const omitted = normalized.filter(Exit.isFailure).length
const output = {
  ...rawOutput,
  output: omitted === 0 ? rawOutput.output
    : `${rawOutput.output}\n\n[${omitted} image${omitted === 1 ? "" : "s"} omitted: could not be resized below the image size limit.]`,
  attachments: normalized.filter(Exit.isSuccess).map((item) => item.value),
}
yield* completeToolCall(value.id, output)
```

**OpenCode tool part 状态机**：
```
ensureToolCall     → status: "pending",  input: {}
tool-call          → status: "running",  input: {实际参数}
tool-result(ok)    → status: "completed", output, title, metadata, attachments
tool-result(err)   → status: "error",    error: msg
tool-error         → status: "error",    error: msg（SDK 独立错误，不经 tool-result）
cleanup (中断)     → status: "error",    error: "Tool execution aborted", interrupted: true
```

**额外行为（Loom 待对齐）**：

| 行为 | OpenCode 处理 | Loom 当前 | 建议 |
|---|---|---|---|
| `failToolCall` 权限拒绝 | 设置 `ctx.blocked = ctx.shouldBreak`，控制循环终止 | 无等价逻辑 | **应对齐**：agent loop 需根据权限拒绝决定是否终止 |
| `tool-error` 事件 | 独立 handler 调用 `failToolCall` | 未处理 | **应对齐**：SDK 可能直接发 `tool-error` |
| `tool-result` 图片归一化 | `image.normalize()` + omitted 统计 | 无 | Loom 暂无图片附件机制，暂不需要 |
| `ensureToolCall` providerExecuted | 已存在 part 收到标记时更新 metadata | 无 | 低优先级，仅影响 provider-side execution 场景 |

#### Loom 当前代码

```rust
// translator.rs:210-292 — create_or_update_tool_part + apply_transition
// 状态机：Create → Start → AppendOutput → Finish
// 与 OpenCode 对齐，已有独立状态管理
```

Loom 的 tool part 管理已经与 OpenCode 基本对齐（独立 ID、状态机转换、call_id 匹配），
**不需要修改**。

唯一差异：

| 方面 | OpenCode | Loom 当前 |
|---|---|---|
| Pending 时 input | `{}` 空对象 | 实际参数 |
| Running 时 input | 实际参数 | 不变（沿用 Create 时传入的参数） |
| Pending 状态 `raw` | `""` 空字符串 | 实际参数的 JSON string |

这些差异不影响正确性，只是时间点略有不同。Loom 在 `ToolCall`（`tool-input-start` 等价）
时就传入了实际参数，OpenCode 在 `tool-call` 时才设置。

---

### 2.7 Event 协议：updatePart vs updatePartDelta

#### OpenCode 处理

OpenCode 使用两种事件发送文本增量：

```typescript
// updatePart — 完整 part 内容
yield* session.updatePart(part)
// 触发事件：message.part.updated（带完整 part 对象）

// updatePartDelta — 仅增量文本
yield* session.updatePartDelta({
  sessionID, messageID, partID,
  field: "text",
  delta: value.text,  // 只有新增的文本片段
})
// 触发事件：message.part.delta（带 partID + field + delta）
```

**OpenCode 的使用策略**：

| 事件 | 使用场景 |
|---|---|
| `updatePart` | Part 创建（`*-start`）、收尾（`*-end`）、tool 状态变更 |
| `updatePartDelta` | 文本流式追加（`*-delta`）—— 每次只发送新增文本 |

事件 schema 定义（`packages/schema/src/v1/session.ts`）：

```typescript
// message.part.updated — 完整 part 替换
PartUpdated: define({
  type: "message.part.updated",
  schema: {
    sessionID: SessionID,
    part: Part,          // 完整的 Part 对象
    time: Schema.Finite, // 服务端时间戳
  },
})

// message.part.delta — 文本增量追加
PartDelta: define({
  type: "message.part.delta",
  schema: {
    sessionID: SessionID,
    messageID: MessageID,
    partID: PartID,
    field: Schema.String, // "text"
    delta: Schema.String, // 新增的文本片段
  },
})
```

> **注**：计划早期版本引用了 `event-reducer.ts` 中的客户端 Binary.search 插入逻辑，
> 但该文件在当前 OpenCode 源码中不存在。客户端（TUI / OpenChamber）的事件消费
> 实现可能位于各自的前端项目中，不属于 opencode 后端仓库。

#### Loom 当前代码

```rust
// translator.rs:438-446 — 每次 delta 都发送完整 part
emit(
    state,
    "message.part.updated",
    json!({
        "sessionID": session_id,
        "part": payload,  // ← 完整 part 对象（累积文本）
        "time": chrono::Utc::now().timestamp_millis(),
    }),
);
```

**Loom 使用 `message.part.updated` 发送累积文本**，而非 OpenCode 的 `message.part.delta`。

#### Loom 修改方案

**短期（保持现状）**：继续用 `message.part.updated` 发送累积文本。
原因：

1. 客户端 `message.part.updated` handler 做 `reconcile(part)` 整体替换，
   累积文本和增量文本在 UI 渲染上等价
2. `message.part.delta` 事件需要客户端额外实现 delta 累加器
3. Loom 当前缺少 `message.part.delta` 事件定义

**中期（可选优化）**：

在追加路径（`translate_chunk` 的 else 分支）中，可以额外发送 `message.part.delta`：

```rust
// 增量路径——发送 delta 事件而非完整 part
emit(
    state,
    "message.part.delta",
    json!({
        "sessionID": session_id,
        "messageID": assistant_msg_id,
        "partID": a.part_id,
        "field": "text",
        "delta": chunk.content,  // ← 只发新增文本
    }),
);
```

**但需要先确认客户端支持该事件**。OpenCode 后端确实会发出 `message.part.delta`
事件（`session.ts` 中 `updatePartDelta`），但客户端（TUI / OpenChamber）是否实现了
delta 累加器需要在前端代码中确认。

---

### 2.8 PartID 生成策略

#### OpenCode 处理

```typescript
// packages/opencode/src/session/schema.ts
// PartID 基于 ULID 风格的单调递增有序 ID（时间戳 hex + 随机 base62，共 26 字节）
export const PartID = Schema.String.check(Schema.isStartsWith("prt")).pipe(
  Schema.brand("PartID"),
  statics((s) => ({
    ascending: (id?: string) => s.make(Identifier.ascending("part", id)),
  })),
)

// Identifier.ascending("part") 生成格式：prt_<6字节hex时间戳 + 14字节随机base62>
// 不是纯数字，而是 ULID 风格的有序字符串（packages/opencode/src/id/id.ts）
```

#### Loom 当前代码

```rust
// state.rs — new_part_id()
pub fn new_part_id() -> String {
    format!("prt_{}", uuid::Uuid::new_v4())
}
```

Loom 使用 `prt_<uuid>`（随机），OpenCode 使用 `prt_<hex时间戳+random_base62>`（ULID 风格单调递增）。

#### Loom 修改方案

**不需要修改**。客户端的 `event-reducer.ts` 对两种 ID 都能正确处理：
- 新 part → Binary.search 找不到 → 按排序位置插入
- 已有 part → Binary.search 找到 → `reconcile` 替换

UUID 的无序性不影响正确性，只影响 parts 列表的显示顺序（但 Loom 用 push 追加到列表末尾，
不依赖 ID 排序）。如果需要严格对齐，可改为：

```rust
// 可选：使用 AtomicI64 生成单调递增 ID
use std::sync::atomic::{AtomicI64, Ordering};

static PART_SEQ: AtomicI64 = AtomicI64::new(0);

pub fn new_part_id() -> String {
    let seq = PART_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("prt_{}", seq)
}
```

---

### 2.9 step-start / step-finish 事件

#### OpenCode 处理

```typescript
// step-start：插入 step-start part（标记一个 LLM 调用回合开始）
// processor.ts:422-429
case "step-start":
  if (!ctx.snapshot) ctx.snapshot = yield* snapshot.track()
  yield* session.updatePart({
    id: PartID.ascending(),
    messageID: ctx.assistantMessage.id,
    sessionID: ctx.sessionID,
    snapshot: ctx.snapshot,
    type: "step-start",
  })
  return

// step-finish：插入 step-finish part，收尾所有 reasoning
// processor.ts:431-468
case "step-finish": {
  const completedSnapshot = yield* snapshot.track()
  yield* Effect.forEach(Object.keys(ctx.reasoningMap), finishReasoning)  // 批量收尾
  const usage = Session.getUsage({
    model: ctx.model, usage: value.usage ?? new Usage({}), metadata: value.providerMetadata,
  })
  ctx.assistantMessage.finish = value.reason
  ctx.assistantMessage.cost += usage.cost
  ctx.assistantMessage.tokens = usage.tokens
  yield* session.updatePart({
    id: PartID.ascending(),
    messageID: ctx.assistantMessage.id,
    sessionID: ctx.assistantMessage.sessionID,
    reason: value.reason, snapshot: completedSnapshot, type: "step-finish",
    tokens: usage.tokens, cost: usage.cost,
  })
  yield* session.updateMessage(ctx.assistantMessage)

  // ── patch part：提交文件变更快照 ──
  if (ctx.snapshot) {
    const patch = yield* snapshot.patch(ctx.snapshot)
    if (patch.files.length) {
      yield* session.updatePart({
        id: PartID.ascending(), messageID, sessionID,
        type: "patch", hash: patch.hash, files: patch.files,
      })
    }
    ctx.snapshot = undefined
  }

  // ── summary fork：异步生成会话摘要 ──
  yield* summary.summarize({ sessionID, messageID: ctx.assistantMessage.parentID })
    .pipe(Effect.ignore, Effect.forkIn(scope))

  // ── compaction check：token 溢出时设置标记 ──
  if (!ctx.assistantMessage.summary &&
      isOverflow({ cfg: yield* config.get(), tokens: usage.tokens, model: ctx.model })) {
    ctx.needsCompaction = true
  }
  return
}
```

`step-start` / `step-finish` 事件作为 part 插入到消息中，在 TUI 中标记回合边界。

**`step-finish` 的完整职责**（上述代码已展开）：
1. 批量收尾所有活跃 reasoning（`finishReasoning`）
2. 计算 token 用量，更新 `assistantMessage.cost` / `.tokens`
3. 插入 `step-finish` part（含 reason、snapshot、tokens、cost）
4. **patch part**：提交文件变更快照（如果有变更）
5. **summary fork**：异步生成会话摘要（不阻塞主流程）
6. **compaction check**：token 超限时设置 `needsCompaction`，触发 `Stream.takeUntil` 提前终止

#### Loom 当前代码

Loom 不生成 `step-start` / `step-finish` part。
LLM 的每轮调用由 `translate_stream_event` 中的事件序列隐式表示。

#### Loom 修改方案

**短期不实现**。`step-start` / `step-finish` part 的主要用途：
1. TUI 渲染回合分隔线
2. 快照（snapshot）追踪
3. Token 用量统计

Loom 已经通过 `message.tokens` 事件处理 token 用量。
回合分隔可以在后续作为增强功能添加。

---

### 2.10 错误处理与重试管线

#### OpenCode 处理

OpenCode 的 `process()` 不是简单的 stream drain，而是一个完整的 Effect 管线：

```typescript
const process = Effect.fn("SessionProcessor.process")(function* (streamInput) {
  ctx.needsCompaction = false
  ctx.shouldBreak = (yield* config.get()).experimental?.continue_loop_on_deny !== true

  return yield* Effect.gen(function* () {
    yield* Effect.gen(function* () {
      ctx.currentText = undefined
      ctx.reasoningMap = {}
      yield* status.set(ctx.sessionID, { type: "busy" })
      const stream = llm.stream(streamInput)

      yield* stream.pipe(
        Stream.tap((event) => handleEvent(event)),
        Stream.takeUntil(() => ctx.needsCompaction),  // ← compaction 时提前终止
        Stream.runDrain,
      )
    }).pipe(
      // 中断时：标记 aborted + 调用 halt
      Effect.onInterrupt(() =>
        Effect.gen(function* () {
          aborted = true
          if (!ctx.assistantMessage.error) {
            yield* halt(new DOMException("Aborted", "AbortError"))
          }
        }),
      ),
      // 非中断异常：进入重试策略
      Effect.catchCauseIf(
        (cause) => !Cause.hasInterruptsOnly(cause),
        (cause) => Effect.fail(Cause.squash(cause)),
      ),
      Effect.retry(
        SessionRetry.policy({
          provider: input.model.providerID,
          parse,
          set: (info) => status.set(ctx.sessionID, {
            type: "retry", attempt: info.attempt, message: info.message,
            action: info.action, next: info.next,
          }),
        }),
      ),
      // 重试用尽后：最终错误兜底
      Effect.catch(halt),
      // 无论成功/失败/中断：总是执行 cleanup
      Effect.ensuring(cleanup()),
    )

    if (ctx.needsCompaction) return "compact"
    if (ctx.blocked || ctx.assistantMessage.error) return "stop"
    return "continue"
  })
})
```

**`halt()` — 错误处理函数**：

```typescript
const halt = Effect.fn("SessionProcessor.halt")(function* (e: unknown) {
  yield* Effect.logError("process", { sessionID, messageID, error: errorMessage(e), stack })
  const error = parse(e)

  // ContextOverflowError 特殊处理
  if (SessionV1.ContextOverflowError.isInstance(error)) {
    if ((yield* config.get()).compaction?.auto === false && !ctx.assistantMessage.summary) {
      // auto-compaction 关闭 → 直接报错
      ctx.assistantMessage.error = error
      ctx.assistantMessage.finish = "error"
      yield* events.publish(Session.Event.Error, { sessionID, error })
      yield* status.set(ctx.sessionID, { type: "idle" })
      return
    }
    // auto-compaction 开启 → 标记需要压缩
    ctx.needsCompaction = true
    yield* events.publish(Session.Event.Error, { sessionID, error })
    return
  }

  // 其他所有错误：设置 message.error
  ctx.assistantMessage.error = error
  yield* events.publish(Session.Event.Error, {
    sessionID: ctx.assistantMessage.sessionID, error: ctx.assistantMessage.error,
  })
  yield* status.set(ctx.sessionID, { type: "idle" })
})
```

**管线关键设计**：

| 管线阶段 | 作用 | Loom 等价 |
|---|---|---|
| `Stream.takeUntil(needsCompaction)` | token 溢出时提前终止流 | Loom 无此机制 |
| `Effect.onInterrupt` | 用户中断时标记 `aborted` + 调用 `halt` | Loom 靠 tokio task cancel |
| `Effect.retry(SessionRetry.policy)` | 自动重试（指数退避） | Loom 无重试 |
| `Effect.catch(halt)` | 重试用尽后设置 `message.error` | Loom 直接返回错误 |
| `Effect.ensuring(cleanup)` | 无论结果都执行收尾 | Loom 靠 RAII / drop |
| `process()` 返回值 | `"compact" / "stop" / "continue"` | Loom 无此三态 |

#### Loom 修改方案

Loom 当前缺乏完整的错误处理管线。建议分阶段对齐：

```rust
// agent_runner.rs — run_agent() 中的错误处理（短期）

match run_llm_stream(...).await {
    Ok(_) => {
        // 正常结束
        finalize_active_part(&state, &sid, &assistant_msg_id);
    }
    Err(e) => {
        // ── 等价于 halt() ──
        // 1. 收尾活跃 part（cleanup 的 text/reasoning 部分）
        finalize_active_part(&state, &sid, &assistant_msg_id);

        // 2. 设置 message.error
        let error_msg = format_error(&e);
        set_message_error(&state, &sid, &assistant_msg_id, &error_msg);

        // 3. 发送 error 事件
        emit(&state, "session.error", json!({
            "sessionID": sid,
            "error": { "message": error_msg, ... },
        }));
    }
}

// 无论成功/失败，都需要标记 message completed
mark_message_completed(&state, &sid, &assistant_msg_id);
```

**中长期**：
- `Stream.takeUntil(needsCompaction)` → 在 agent loop 中检查 token 用量，超限时 break
- `Effect.retry` → 在 LLM 调用层添加重试策略
- `process()` 三态返回 → `run_agent` 返回 `Compact / Stop / Continue` 枚举

---

## 三、修改清单汇总

### 3.1 必须修改

| # | 文件 | 修改内容 | 等价于 OpenCode |
|---|---|---|---|
| 1 | `state.rs` | 新增 `ActivePart` 结构 + `SharedState.active` 字段 | `ctx.currentText` / `ctx.reasoningMap` |
| 2 | `translator.rs` | 重写 `translate_chunk`：基于 `ActivePart` 状态驱动 | `text-start`/`text-delta`/`reasoning-start`/`reasoning-delta` |
| 3 | `translator.rs` | 新增 `finalize_active_part` 函数 | `text-end` / `finishReasoning` |
| 4 | `translator.rs` | `ToolCall` 事件中调用 `finalize_active_part` | (OpenCode 靠 `text-end` 先行到达) |
| 5 | `translator.rs` | `close_open_text_parts` 改为调用 `finalize_active_part` | `cleanup()` |
| 6 | `agent_runner.rs` | `run_agent` 开头清除 `state.active` | `process()` 开头重置 `ctx` |
| 7 | `translator.rs` | 处理 `tool-error` 事件（独立于 tool-result） | `case "tool-error"` |
| 8 | `agent_runner.rs` | 错误路径设置 `message.error` + 发送 `session.error` 事件 | `halt()` |

### 3.2 不需要修改

| 文件 | 原因 |
|---|---|
| `translator.rs` tool part 核心逻辑 | 状态机已与 OpenCode 对齐（`create_or_update_tool_part`） |
| `sse.rs` | SSE 序列化逻辑不受影响 |

### 3.3 可选优化（中期）

| # | 修改 | 优先级 |
|---|---|---|
| A | 追加路径发送 `message.part.delta` 事件 | 中 |
| B | PartID 改为单调递增 | 低 |
| C | 新增 `step-start` / `step-finish` part | 低 |
| D | cleanup 中 tool 250ms 宽限期等待在途工具 | 中 |
| E | aborted tool 标记 `interrupted: true` 元数据 | 中 |
| F | `failToolCall` 权限拒绝时设置 `blocked`，控制 agent loop 终止 | 中 |
| G | `step-finish` 生成 patch part（需 snapshot 机制） | 低 |
| H | `step-finish` compaction check（token 溢出检测） | 低 |
| I | LLM 调用层添加重试策略（指数退避） | 低 |

---

## 四、事件序列对比示例

场景：用户发消息 → LLM 先推理 → 输出文本 → 调用工具 → 再推理 → 输出最终文本

### 4.1 OpenCode 事件序列

```
LLM Stream #1:
  reasoning-start(id=r1) → updatePart: 创建 reasoning part prt_1
  reasoning-delta(id=r1) → updatePartDelta: 追加 delta 到 prt_1
  reasoning-end(id=r1)   → updatePart: 设置 prt_1.time.end, 从 reasoningMap 删除 r1
  text-start             → updatePart: 创建 text part prt_2
  text-delta             → updatePartDelta: 追加 delta 到 prt_2
  text-end               → updatePart: 设置 prt_2.time.end, ctx.currentText = undefined
  tool-input-start       → updatePart: 创建 tool part prt_3 (pending)
  tool-call              → updatePart: 更新 prt_3 (running)
  tool-result            → updatePart: 更新 prt_3 (completed)
  step-finish            → updatePart: 创建 step-finish part prt_4

LLM Stream #2:
  reasoning-start(id=r2) → updatePart: 创建 reasoning part prt_5
  reasoning-delta(id=r2) → updatePartDelta: 追加 delta 到 prt_5
  reasoning-end(id=r2)   → updatePart: 设置 prt_5.time.end
  text-start             → updatePart: 创建 text part prt_6
  text-delta             → updatePartDelta: 追加 delta 到 prt_6
  text-end               → updatePart: 设置 prt_6.time.end
  step-finish            → updatePart: 创建 step-finish part prt_7
```

### 4.2 Loom 修改后事件序列（同样的场景）

```
run_agent 开始:
  state.active.remove(msg_id)  // 重置

LLM Stream #1:
  Messages(thinking)     → active=None → 新建 reasoning prt_a, 设为 active
  Messages(thinking)     → active.type=reasoning → 追加到 prt_a
  Messages(text)         → active.type=reasoning ≠ text → finalize prt_a, 新建 text prt_b
  Messages(text)         → active.type=text → 追加到 prt_b
  ToolCall               → finalize prt_b, 创建 tool part tool-call_xxx
  ToolStart              → 更新 tool part → running
  ToolEnd                → 更新 tool part → completed
  (agent loop 可能继续)
```

```
LLM Stream #2（仍在同一个 assistant_msg_id 下）:
  Messages(thinking)     → active=None → 新建 reasoning prt_c, 设为 active
  Messages(thinking)     → active.type=reasoning → 追加到 prt_c
  Messages(text)         → active.type=reasoning ≠ text → finalize prt_c, 新建 text prt_d
  Messages(text)         → active.type=text → 追加到 prt_d

run 结束:
  finalize_active_part   → 收尾 prt_d (time.end)
```

### 4.3 最终 Parts 列表

```
prt_a: reasoning "Let me think..."    time.end ✅
prt_b: text "Running ls..."           time.end ✅
tool-call_xxx: tool "bash" completed  time.end ✅
prt_c: reasoning "Now I see..."       time.end ✅
prt_d: text "The result is..."        time.end ✅
```

---

## 五、数据流图

```
                    ┌─────────────┐
                    │ LLM Stream  │
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │ translate_  │
                    │ and_emit()  │
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
     ┌────────▼───┐ ┌─────▼─────┐ ┌───▼────────┐
     │ Messages   │ │ ToolXxx   │ │  Usage /   │
     │ (chunk)    │ │           │ │  Others    │
     └────────┬───┘ └─────┬─────┘ └────────────┘
              │           │
     ┌────────▼───┐       │
     │ translate_ │       │
     │ chunk()    │       │
     └────────┬───┘       │
              │           │
     ┌────────▼───────────▼────────┐
     │  state.active (HashMap)     │
     │  ┌──────────────────────┐   │
     │  │ msg_xxx → ActivePart │   │
     │  │  part_type: "text"   │   │
     │  │  part_id: "prt_b"    │   │
     │  │  text: "Hello..."    │   │
     │  └──────────────────────┘   │
     └─────────────────────────────┘
              │
     ┌────────▼───────────────────┐
     │  state.parts (HashMap)     │
     │  ┌──────────────────────┐  │
     │  │ msg_xxx → [          │  │
     │  │   {prt_a, reasoning},│  │
     │  │   {prt_b, text},     │  │
     │  │   {tool-x, tool},    │  │
     │  │ ]                    │  │
     │  └──────────────────────┘  │
     └────────────────────────────┘
              │
     ┌────────▼───────────────────┐
     │  SSE emit()                │
     │  message.part.updated      │
     │  message.part.delta (可选) │
     └────────────────────────────┘
```

---

## 六、LLM 流式事件层改造（从根本上消除 ActivePart 推断）

### 6.1 问题根源

§2.1–§2.5 的 `ActivePart` 推断方案是对 Loom 缺少三段式事件的**补偿**。
根本解法是在 LLM streaming 层（`loom-llm` crate）将 provider SSE 原始数据解析为
**显式的 start/delta/end 事件**，使 translator 无需猜测 part 边界。

### 6.2 新增事件类型

```rust
// loom-llm/src/stream_event.rs

pub enum LlmStreamEvent {
    // ── 文本 ──
    TextStart   { metadata: ProviderMetadata },
    TextDelta   { text: String },
    TextEnd     { metadata: ProviderMetadata },

    // ── 推理 ──
    ReasoningStart { id: String, metadata: ProviderMetadata },
    ReasoningDelta { id: String, text: String },
    ReasoningEnd   { id: String, metadata: ProviderMetadata },

    // ── 工具（已有，归入统一枚举）──
    ToolCall   { id: String, name: String, arguments: serde_json::Value },
    ToolResult { id: String, result: serde_json::Value, is_error: bool },
    ToolError  { id: String, error: String },

    // ── 回合 ──
    StepStart,
    StepFinish { reason: String, usage: Usage },

    // ── 终止 ──
    Finish,
    ProviderError { message: String },
}
```

替换当前的 `MessageChunk { kind, content }` 扁平结构。

### 6.3 各 Provider SSE 映射

**Anthropic**（天然三段式，直接映射）：

| Anthropic SSE | → LlmStreamEvent |
|---|---|
| `message_start` | `StepStart` |
| `content_block_start` (type=text) | `TextStart` |
| `content_block_delta` (text_delta) | `TextDelta` |
| `content_block_start` (type=thinking) | `ReasoningStart { id: block.index }` |
| `content_block_delta` (thinking_delta) | `ReasoningDelta { id }` |
| `content_block_stop` | `TextEnd` 或 `ReasoningEnd { id }`（按当前 block 类型） |
| `message_delta` (stop_reason) | `StepFinish` |
| `message_stop` | `Finish` |

**OpenAI**（无三段式，需 parser 内部状态追踪）：

| OpenAI SSE 字段 | → LlmStreamEvent | 状态追踪 |
|---|---|---|
| `delta.content` 首次非空 | `TextStart` | 进入 text block |
| `delta.content` 后续 | `TextDelta` | 维持 text block |
| `delta.reasoning_content` 首次非空 | `ReasoningStart` | 退出 text block（合成 `TextEnd`）→ 进入 reasoning block |
| `delta.reasoning_content` 后续 | `ReasoningDelta` | 维持 reasoning block |
| `delta.content` 在 reasoning 后再次出现 | 先 `ReasoningEnd` → `TextStart` | 退出 reasoning block → 进入 text block |
| `finish_reason` 到达 | `TextEnd` 或 `ReasoningEnd`（按当前 block）→ `StepFinish` | 清空所有 block 状态 |
| `finish_reason: "error"` | `ProviderError` | — |

**关键**：OpenAI parser 需要一个 `BlockTracker` 结构记录"当前在哪个 block 中"，
在类型翻转时合成前一个 block 的 End 事件。

### 6.4 BlockTracker 参考实现（OpenAI compat）

```rust
// loom-llm/src/openai_compat/block_tracker.rs

#[derive(Debug, Clone, PartialEq)]
enum ActiveBlock {
    None,
    Text,
    Reasoning { id: String },
}

pub struct BlockTracker {
    active: ActiveBlock,
    reasoning_seq: usize,
}

impl BlockTracker {
    pub fn new() -> Self {
        Self { active: ActiveBlock::None, reasoning_seq: 0 }
    }

    /// 处理一条 SSE delta，返回 0-2 个 LlmStreamEvent（类型翻转时先 End 旧 block 再 Start 新 block）
    pub fn on_text_delta(&mut self, text: &str, metadata: ProviderMetadata) -> Vec<LlmStreamEvent> {
        let mut events = Vec::new();
        if self.active != ActiveBlock::Text {
            events.extend(self.close_current());
            self.active = ActiveBlock::Text;
            events.push(LlmStreamEvent::TextStart { metadata: metadata.clone() });
        }
        events.push(LlmStreamEvent::TextDelta { text: text.to_string() });
        events
    }

    pub fn on_reasoning_delta(&mut self, text: &str, metadata: ProviderMetadata) -> Vec<LlmStreamEvent> {
        let mut events = Vec::new();
        let id = match &self.active {
            ActiveBlock::Reasoning { id } => id.clone(),
            _ => {
                events.extend(self.close_current());
                let id = format!("r{}", self.reasoning_seq);
                self.reasoning_seq += 1;
                self.active = ActiveBlock::Reasoning { id: id.clone() };
                events.push(LlmStreamEvent::ReasoningStart { id: id.clone(), metadata: metadata.clone() });
                id
            }
        };
        events.push(LlmStreamEvent::ReasoningDelta { id, text: text.to_string() });
        events
    }

    pub fn on_finish(&mut self, metadata: ProviderMetadata) -> Vec<LlmStreamEvent> {
        let mut events = self.close_current();
        events.push(LlmStreamEvent::Finish);
        events
    }

    fn close_current(&mut self) -> Vec<LlmStreamEvent> {
        let mut events = Vec::new();
        match std::mem::replace(&mut self.active, ActiveBlock::None) {
            ActiveBlock::Text => events.push(LlmStreamEvent::TextEnd { metadata: ProviderMetadata::default() }),
            ActiveBlock::Reasoning { id } => events.push(LlmStreamEvent::ReasoningEnd { id, metadata: ProviderMetadata::default() }),
            ActiveBlock::None => {}
        }
        events
    }
}
```

### 6.5 Translator 简化效果

有了 `LlmStreamEvent` 后，§2.1 的 `ActivePart` 推断逻辑**完全删除**：

```rust
// translator.rs — 新版 translate_stream_event

fn translate_stream_event(event: &LlmStreamEvent, session_id: &str, msg_id: &str, state: &SharedState) {
    match event {
        LlmStreamEvent::TextStart { metadata } => {
            // 直接创建新 part，无需检查 active 状态
            let part_id = new_part_id();
            push_part(state, msg_id, session_id, "text", json!({
                "id": part_id, "type": "text", "text": "",
                "time": { "start": now_ms() }, "metadata": metadata,
            }));
            set_active_text(state, msg_id, part_id);
        }

        LlmStreamEvent::TextDelta { text } => {
            append_to_active(state, msg_id, text);  // 直接追加，无需类型判断
            emit_delta(state, session_id, msg_id, text);
        }

        LlmStreamEvent::TextEnd { metadata } => {
            finalize_active_part(state, session_id, msg_id);  // 收尾
        }

        LlmStreamEvent::ReasoningStart { id, metadata } => {
            // 直接创建，支持并发 reasoning blocks（如有需要）
            let part_id = new_part_id();
            push_reasoning_part(state, msg_id, session_id, part_id, id, metadata);
        }

        // ... 其余变体一一对应
    }
}
```

与 OpenCode 的 `case "text-start": ... case "text-end": ...` **一一对应**，
不再需要 `translate_chunk` 的 4 步推断逻辑。

### 6.6 改动范围

| 层 | 文件 | 改动 | 工作量 |
|---|---|---|---|
| LLM streaming | `loom-llm/src/openai_compat/stream.rs` | SSE parser 内嵌 `BlockTracker`，输出 `LlmStreamEvent` | **大** |
| LLM streaming | `loom-llm/src/openai_compat/block_tracker.rs` | 新建 `BlockTracker` 结构 | 中 |
| LLM streaming | `loom-llm/src/anthropic_compat/stream.rs` | `content_block_*` 直接映射 `LlmStreamEvent` | **小** |
| LLM streaming | `loom-llm/src/stream_event.rs` | 新建 `LlmStreamEvent` 枚举 | 小 |
| Translator | `translator.rs` | `translate_stream_event` 改为匹配 `LlmStreamEvent` 变体 | 中 |
| Translator | `translator.rs` | 删除 `ActivePart` / `translate_chunk` 推断逻辑 | 小（删除） |
| Agent | `agent_runner.rs` | stream loop 遍历 `LlmStreamEvent` 而非 `MessageChunk` | 中 |

### 6.7 迁移策略

**阶段 1（短期）**：先实现 §2.1–§2.5 的 `ActivePart` 推断方案（不改动 LLM 层），
快速解决当前 bug。

**阶段 2（中期）**：在 LLM 层引入 `LlmStreamEvent` + `BlockTracker`，
然后简化 translator 删除 `ActivePart` 推断。

两阶段的好处：阶段 1 的 `ActivePart` 可以作为阶段 2 的**过渡兼容层**——
如果 `LlmStreamEvent` 解析出错（某 provider 的 SSE 格式异常），
可以 fallback 到 `ActivePart` 推断模式。
