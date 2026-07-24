# Text/Reasoning Part 生命周期

> 返回 [README.md](README.md)

## 2.1 Text Part 生命周期

> 开发任务：E1（active_text 状态）、E3（TextBlockStart arm）、E4（TextDelta arm）、E5（TextBlockEnd arm）、E15（finalize_text_part）

### OpenCode 处理

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

### Loom 当前代码（有问题）

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
- 正常 `text → reasoning → text` 序列恰好能工作（last 类型不匹配 → 创建新 part）
- **真正的 bug**：错误中断 / agent loop 多轮调用时，活跃 part 未被收尾，残留状态泄漏到下一轮

### Loom 修改方案

删除 `translate_chunk`；改为 `TextBlockStart` / `TextDelta` / `TextBlockEnd` 三个 match arm，配合 `active_text[msg_id] → part_id` 映射：

```rust
// state.rs — 新增字段
// SharedState = Arc<AppState>（state.rs:87）
// AppState 新增两个 RwLock 字段：

pub struct AppState {
    // ...existing 15 fields...

    /// 当前活跃的 text part（按 message_id → part_id 索引）
    pub active_text: RwLock<HashMap<String, String>>,

    /// 当前活跃的 reasoning parts（按 message_id → reasoning_id → part_id 索引）
    pub active_reasoning: RwLock<HashMap<String, HashMap<String, String>>>,
}
```

```rust
// translator.rs — TextBlockStart arm
// push_part 定义在 agent_runner.rs:160，签名为：
//   pub fn push_part(state: &SharedState, message_id: &str, session_id: &str, part_type: &str, mut data: Value)
// emit 定义在 state.rs:508，签名为：
//   pub fn emit(state: &SharedState, event_type: &str, properties: serde_json::Value)

StreamEvent::TextBlockStart { metadata } => {
    let part_id = new_part_id();
    push_part(state, msg_id, session_id, "text", json!({
        "id": part_id, "type": "text", "text": "",
        "time": { "start": now_ms(), "created": now_ms() },
        "metadata": metadata,
    }));
    state.active_text.write().insert(msg_id.to_string(), part_id);
}

// translator.rs — TextDelta arm
// 当前代码无 emit_part_updated / append_to_part 辅助函数；
// 需新增或内联 state.parts.write() + emit() 逻辑。
StreamEvent::TextDelta { content, metadata } => {
    let part_id = state.active_text.read().get(msg_id).cloned();
    if let Some(pid) = part_id {
        // 内联追加（与当前 translate_chunk L436-470 一致的模式）
        let payload = {
            let mut parts = state.parts.write();
            parts.get_mut(msg_id).and_then(|list| list.iter_mut().find(|p| p.id == pid)).map(|p| {
                let text = p.data["text"].as_str().unwrap_or("").to_string();
                p.data["text"] = json!(text + &content);
                p.data.clone()
            })
        };
        if let Some(payload) = payload {
            emit(state, "message.part.updated", json!({
                "sessionID": session_id, "part": payload, "time": now_ms(),
            }));
        }
    }
}

// translator.rs — TextBlockEnd arm
StreamEvent::TextBlockEnd { .. } => {
    finalize_text_part(state, session_id, msg_id);
    state.active_text.write().remove(msg_id);
}
```

---

## 2.2 Reasoning Part 生命周期

> 开发任务：E1（active_reasoning 状态）、E6（ReasoningBlockStart arm）、E7（ReasoningDelta arm）、E8（ReasoningBlockEnd arm）、E15（finalize_reasoning_part）

### OpenCode 处理

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

### Loom 当前代码

```rust
// translator.rs:416-418 — reasoning 和 text 共用同一段逻辑
let part_type = if chunk.is_thinking() {
    "reasoning"
} else {
    "text"
};
// 然后走与 text 完全相同的 last_mut 匹配路径
```

Loom 不区分 reasoning 和 text 的处理路径，共用 `translate_chunk`（当前问题）。

### Loom 修改方案

与 text 对称，使用 `ReasoningBlockStart` / `ReasoningDelta` / `ReasoningBlockEnd` 三个 arm + `active_reasoning[msg_id][reasoning_id] → part_id` 双层映射：

```rust
// translator.rs — ReasoningBlockStart arm
StreamEvent::ReasoningBlockStart { id, metadata } => {
    let part_id = new_part_id();
    push_part(state, msg_id, session_id, "reasoning", json!({
        "id": part_id, "type": "reasoning", "text": "",
        "time": { "start": now_ms(), "created": now_ms() },
        "metadata": metadata,
    }));
    state.active_reasoning.write()
        .entry(msg_id.to_string()).or_default()
        .insert(id.clone(), part_id);
}

// translator.rs — ReasoningDelta arm
StreamEvent::ReasoningDelta { id, content, metadata } => {
    let part_id = state.active_reasoning.read()
        .get(msg_id).and_then(|m| m.get(id)).cloned();
    if let Some(pid) = part_id {
        let payload = {
            let mut parts = state.parts.write();
            parts.get_mut(msg_id).and_then(|list| list.iter_mut().find(|p| p.id == pid)).map(|p| {
                let text = p.data["text"].as_str().unwrap_or("").to_string();
                p.data["text"] = json!(text + &content);
                p.data.clone()
            })
        };
        if let Some(payload) = payload {
            emit(state, "message.part.updated", json!({
                "sessionID": session_id, "part": payload, "time": now_ms(),
            }));
        }
    }
}

// translator.rs — ReasoningBlockEnd arm
StreamEvent::ReasoningBlockEnd { id, .. } => {
    finalize_reasoning_part(state, session_id, msg_id, &id);
    if let Some(parts) = state.active_reasoning.write().get_mut(msg_id) {
        parts.remove(&id);
    }
}
```

**与 text 的区别**：reasoning 支持按 `id` 并发多个 block（`active_reasoning` 是双层 map），text 同一时刻只有一个活跃 part（`active_text` 是单层 map）。

---

## 2.3 Tool Part 与 Text/Reasoning 的边界

> 开发任务：E11（ToolCall arm 移除 finalize）

### OpenCode 处理

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

// provider-error：直接 throw，进入 retry/halt 管线（见 05-error-handling.md）
case "provider-error":
  throw new Error(value.message)

// finish：显式 no-op
case "finish":
  return
```

OpenCode 不在 tool 事件中收尾 text/reasoning——它依赖 `text-end` 事件已经先行到达。
LLM SDK 的事件顺序保证了 `text-end` → `tool-input-start` 的时序。

### Loom 修改方案

**当前**：Loom 的 stream 事件顺序是 `Messages(text) → ToolCall → Messages(text) ...`，
没有显式 `text-end`，因此 tool 事件必须触发收尾。**改造后** 改为
`TextBlockStart → TextDelta → TextBlockEnd → ToolCall`，ToolCall arm 不再收尾（见 08 §8.4）。

```rust
// translator.rs — translate_stream_event 中

StreamEvent::ToolCall { call_id, name, arguments } => {
    // block end 事件已先行到达，无需额外收尾

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

## 2.4 Run 结束时的收尾（cleanup）

> 开发任务：E10（TurnFinish arm 中 finalize_all_reasoning_parts）、E15（finalize 函数）、E16（删除 close_open_text_parts）、F1（调用方更新）

### OpenCode 处理

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

**OpenCode cleanup 的执行时机与管线**（见 [05-error-handling.md](05-error-handling.md) 详述）：
- `Effect.ensuring(cleanup())` — 在 `process()` 的 finally 块中执行
- 无论正常结束、异常、中断，cleanup 都会执行
- cleanup 中的 tool 250ms 宽限期允许在途工具（如 `execute` 的子调用）自然完成后再标记为 aborted

### Loom 当前代码

```rust
// translator.rs:353-420 — close_open_text_parts()
// 遍历 parts 列表，为所有没有 time.end 的 text/reasoning part 补上
```

### Loom 修改方案

删除 `close_open_text_parts`，替换为三个收尾函数：

```rust
// translator.rs — 新版收尾函数

/// 收尾活跃 text part（等价于 OpenCode text-end）。
pub fn finalize_text_part(state: &AppState, session_id: &str, msg_id: &str) {
    let part_id = state.active_text.write().remove(msg_id);
    let Some(pid) = part_id else { return };
    finalize_part_by_id(state, session_id, msg_id, &pid);
}

/// 收尾指定 reasoning part（等价于 OpenCode finishReasoning(id)）。
pub fn finalize_reasoning_part(
    state: &AppState, session_id: &str, msg_id: &str, reasoning_id: &str,
) {
    let part_id = state.active_reasoning.write()
        .get_mut(msg_id).and_then(|m| m.remove(reasoning_id));
    let Some(pid) = part_id else { return };
    finalize_part_by_id(state, session_id, msg_id, &pid);
}

/// 收尾所有活跃 reasoning parts（等价于 OpenCode step-finish 中的批量 finishReasoning）。
pub fn finalize_all_reasoning_parts(state: &AppState, session_id: &str, msg_id: &str) {
    let map = state.active_reasoning.write().remove(msg_id);
    let Some(ids) = map else { return };
    for (_, pid) in ids {
        finalize_part_by_id(state, session_id, msg_id, &pid);
    }
}

fn finalize_part_by_id(state: &AppState, session_id: &str, msg_id: &str, part_id: &str) {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let payload = {
        let mut parts = state.parts.write();
        if let Some(list) = parts.get_mut(msg_id) {
            if let Some(p) = list.iter_mut().find(|p| p.id == part_id) {
                if let Some(t) = p.data.get_mut("time").and_then(|v| v.as_object_mut()) {
                    t.insert("end".into(), json!(now_ms));
                    t.insert("completed".into(), json!(now_ms));
                }
                Some(p.data.clone())
            } else { None }
        } else { None }
    };
    if let Some(payload) = payload {
        emit(state, "message.part.updated", json!({
            "sessionID": session_id, "part": payload, "time": now_ms,
        }));
    }
}
```

Session handler 中的调用变更：

```rust
// handlers/session.rs — run_prompt / run_shell 结束处
// 旧：
close_open_text_parts(&state_bg, &sid, &assistant_message_id, ended_at_ms);

// 新：
finalize_text_part(&state_bg, &sid, &assistant_message_id);
finalize_all_reasoning_parts(&state_bg, &sid, &assistant_message_id);
```

**OpenCode cleanup 额外职责（Loom 待考虑对齐）**：

| # | OpenCode cleanup 行为 | Loom 当前 | 建议 |
|---|---|---|---|
| A | snapshot patch part 生成 | 无 | Loom 无 snapshot 机制，暂不需要 |
| B | tool 250ms 宽限期等待在途工具完成 | 直接标记 aborted | **应对齐**：避免在途工具被误标为 error |
| C | aborted tool 标记 `interrupted: true` | 无此元数据 | **应对齐**：前端可区分中断 vs 真实错误 |
| D | tool error 文案为 `"Tool execution aborted"` | 可能不同 | 统一文案以便前端处理 |

---

## 2.5 Run 开始时的状态重置

> 开发任务：G2（清除 active_text / active_reasoning）

### OpenCode 处理

```typescript
// processor.ts:498-499 — process() 函数开头
ctx.currentText = undefined
ctx.reasoningMap = {}
```

每次调用 `process()`（等价于 Loom 的一次 `run_agent`）时，
先清空活跃 part 状态。这保证了：
- 上一次 run 的残留状态不会泄漏到新 run
- 即使 `text-end` 事件丢失（异常 / 中断），新 run 也不会追加到旧 part

### Loom 修改方案

```rust
// agent_runner.rs — run_agent() 开头

pub async fn run_agent(
    state: AppState,
    session_id: String,
    message_id: String,
    workdir: PathBuf,
    prompt: String,
    model: Option<String>,
    agent_name: Option<String>,
) {
    // ── 重置活跃 part 状态 ──
    // 等价于 OpenCode processor.ts:498-499 的 ctx.currentText = undefined; ctx.reasoningMap = {}
    state.active_text.write().remove(&message_id);
    state.active_reasoning.write().remove(&message_id);

    // ...rest of run_agent...
}
```
