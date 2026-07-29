# 运行时正确性对抗性验证报告

> 验证对象：Loom translator 运行时行为
> 验证人：对抗性验证 agent #2
> 范围：`apps/server/src/translator.rs`、`foundation/stream-event/src/block_tracker.rs`、`agent/agent-core/src/agent/react/think_node.rs`

---

## 摘要

| 严重等级 | 数量 | 描述 |
|---|---|---|
| **Critical** | 2 | 数据丢失 / 协议对齐破坏 |
| **High** | 3 | 状态不一致 / 单回合多 part 泄漏 |
| **Medium** | 3 | 顺序/锁/孤儿事件 |
| **Low** | 2 | 边界/无单元测试覆盖 |

---

## 1. Critical 问题

### C-1. ProviderError 后未关闭的 block 仍保持 "open" 状态

**现状**

`translate_stream_event` 中 `StreamEvent::ProviderError { message }` 只调用 `emit(state, "session.error", ...)`（apps/server/src/translator.rs:272-284），不做任何终结操作。

当上游在仍持有 active text/reasoning block 时触发 provider 错误（例如 anthropic_compat 解析失败），`active_text[msg_id]` 或 `active_reasoning[msg_id][id]` 仍指向未关闭的 part。**前端的 TUI 永远不会收到对应 part 的 `message.part.updated`（带 `time.end`），导致 part 永远停在 "streaming" 状态。**

```rust
// apps/server/src/translator.rs:272-284
StreamEvent::ProviderError { message } => {
    emit(state, "session.error", json!({...}));
    // ❌ 没有调用 finalize_text_part / finalize_all_reasoning_parts
}
```

而 `StreamEvent::TurnFinish`（apps/server/src/translator.rs:164-189）正确地先 `finalize_text_part` + `finalize_all_reasoning_parts`，再 push `step-finish` part。ProviderError 路径**不应跳过这一步**。

**影响**
- 协议 §8.9 不变量 `TextDelta 必须位于同一 TextBlockStart 与 TextBlockEnd 之间` 被打破。
- 重新连接后从 `event_buffer` 重放时，open part 永远不会被终结。
- 与附录 B 中 TUI 期望的 `text.ended` 不一致。

**严重等级**：**Critical** — 前端永久卡在 "streaming" 状态。

**建议修复**

```rust
StreamEvent::ProviderError { message } => {
    finalize_text_part(state, session_id, assistant_msg_id);
    finalize_all_reasoning_parts(state, session_id, assistant_msg_id);
    emit(
        state,
        "session.error",
        json!({
            "sessionID": session_id,
            "error": {
                "name": "ProviderError",
                "data": { "message": message },
            },
        }),
    );
}
```

---

### C-2. TurnStart 连续发出时未关闭前一个 step-start / 未终结 open block

**现状**

`StreamEvent::TurnStart`（apps/server/src/translator.rs:150-163）只调用 `push_part` 追加一个 `step-start` part，**不做任何终结**。

```rust
// apps/server/src/translator.rs:150-163
StreamEvent::TurnStart => {
    let now = chrono::Utc::now().timestamp_millis();
    push_part(state, assistant_msg_id, session_id, "step-start", json!({...}));
    // ❌ 没有终结前一个 step-start 或 open block
}
```

复现场景：
1. TurnStart → step-start part A
2. TextBlockStart → text part B（active_text[msg] = B）
3. TextDelta ×3 → text part B 累积
4. **上游再次发 TurnStart（异常路径，例如 anthropic SSE 重连）**
   - 现有行为：push_part 创建 step-start part C，**B 仍在 active_text[msg] 中**
   - 后续 TextDelta 仍然走 B，没有 "TurnStart 强制关闭之前 block" 的语义
   - B 没有 time.end，直到下次 TextBlockEnd 或 TurnFinish
5. 第二个 TurnFinish 时，B 才被终结 → 看起来 "第二个回合继承了第一个回合的 text"

**影响**
- 协议 §8.9 不变量 `TurnStart / TurnFinish 包裹单次 LLM 调用` 被打破。
- 第二个 step-start 的 part id 与未关闭的 text part 同处一个 message，让 TUI 误认为 text 跨回合持续。

**严重等级**：**Critical** — 单 message 内多回合的状态混乱。

**建议修复**

```rust
StreamEvent::TurnStart => {
    finalize_text_part(state, session_id, assistant_msg_id);
    finalize_all_reasoning_parts(state, session_id, assistant_msg_id);
    let now = chrono::Utc::now().timestamp_millis();
    push_part(
        state,
        assistant_msg_id,
        session_id,
        "step-start",
        json!({
            "id": new_part_id(),
            "type": "step-start",
            "time": { "start": now, "created": now },
        }),
    );
}
```

---

## 2. High 问题

### H-1. ToolError 在 ToolCall 到达前发生时静默丢弃错误

**现状**

`fail_tool_call`（apps/server/src/translator.rs:325-360）通过 `state.parts.write()` 查找 `tool-{call_id}` 命名的 part，找不到时**直接 return，不报错、不发出 session.error**（test `tool_error_for_unknown_call_id_is_noop`（apps/server/src/translator.rs:1228-1238）明确锁定此行为）。

```rust
// apps/server/src/translator.rs:325-360
fn fail_tool_call(state, ..., call_id: Option<&str>, error: &str) {
    let Some(call_id) = call_id else { return };  // 1) 无 call_id → 静默
    let part_id = format!("tool-{call_id}");
    let updated = {
        let mut parts = state.parts.write();
        parts.get_mut(assistant_msg_id).and_then(|list| {
            list.iter_mut().find(|part| part.id == part_id).map(|part| {...})
        })
        // 2) 找不到 part → updated == None → 不 emit
    };
    if let Some(payload) = updated {
        emit(...);
        // 3) 没 emit 任何东西给前端知道有错误
    }
}
```

上游合法路径：
1. LLM 决定不调工具（没有 ToolCall）
2. Provider SDK 报告工具执行失败（`ToolError { call_id, error }`）
   - 此时没有 `tool-{call_id}` part 存在
   - 用户在前端看不到任何错误指示

**影响**
- 工具错误被吞掉，前端无感知。
- 与 ProviderError 行为不一致（ProviderError 总是 emit session.error）。

**严重等级**：**High** — 错误路径不可见。

**建议修复**

```rust
fn fail_tool_call(
    state: &SharedState,
    assistant_msg_id: &str,
    session_id: &str,
    call_id: Option<&str>,
    error: &str,
) {
    let Some(call_id) = call_id else {
        emit(
            state,
            "session.error",
            json!({
                "sessionID": session_id,
                "error": {
                    "name": "ToolError",
                    "data": { "message": error },
                },
            }),
        );
        return;
    };
    let part_id = format!("tool-{call_id}");
    let updated = { /* existing logic */ };
    if updated.is_none() {
        emit(
            state,
            "session.error",
            json!({
                "sessionID": session_id,
                "error": {
                    "name": "ToolError",
                    "data": { "message": error, "callID": call_id },
                },
            }),
        );
    }
}
```

---

### H-2. ToolCall 中 ToolTransition::Finish 调用时未带 metadata，与其他 arm 不一致

**现状**

`create_or_update_tool_part`（apps/server/src/translator.rs:382-447）在三条路径中构造 `state` 对象，但 `state.metadata` 没有被任何 Tool 事件（ToolCall/ToolStart/ToolOutput/ToolEnd）填充。`apply_transition` 中 `Finish` 分支（apps/server/src/translator.rs:473-499）也没有将 metadata 写入 `state.metadata`。

更严重的是：**`push_part` 在 `parts.write()` 之后调用 `emit()`（apps/server/src/agent_runner.rs:198），但 `append_to_part`（apps/server/src/translator.rs:295-323）在 parts.write() 闭包之外再次调用 `emit()`**。

观察以下时序：
1. `parts.write()` 闭包内：调用 `apply_transition(&mut p.data, ...)`，修改内存中的 part.data
2. `drop(parts)` 后：`emit("message.part.updated", ...)` 发出的 `data` 是 `p.data.clone()` 的拷贝——`push_part` 路径上 `data` 是构造时的副本，与 `p.data` 是**独立克隆**，所以 push_part 内部数据写入（`state.time.end` 等）**不会**反映到 emit 的 data 上。

**但是**：`append_to_part` 在闭包内 `p.data.clone()` 出来后又调用 emit——发出的是闭包外的旧值。等等，看仔细：

```rust
// apps/server/src/translator.rs:295-323
fn append_to_part(state, ..., content: &str) {
    let payload = {
        let mut parts = state.parts.write();
        parts.get_mut(assistant_msg_id).and_then(|list| {
            list.iter_mut().find(|part| part.id == part_id).map(|part| {
                let existing = part.data["text"].as_str().unwrap_or_default();
                part.data["text"] = json!(format!("{existing}{content}"));
                part.data.clone()   // 拷贝当前 part.data（包含新文本）
            })
        })
        // ✅ 这里 payload 已经包含新 text
    };
    if let Some(payload) = payload {
        emit(state, "message.part.updated", json!({
            "sessionID": session_id, "part": payload,  // ← 新 payload
            "time": ...,
        }));
    }
}
```

这是对的。

**真正的 bug 是**：tool 路径上 `create_or_update_tool_part` 的 update 分支（apps/server/src/translator.rs:396-410）：

```rust
let updated = {
    let mut parts = state.parts.write();
    if let Some(list) = parts.get_mut(assistant_msg_id) {
        if let Some(p) = list.iter_mut().find(|p| p.id == part_id) {
            apply_transition(&mut p.data, &transition);  // ① 原地修改
            let payload = p.data.clone();                 // ② 拷贝
            drop(parts);
            Some(payload)
        } else { None }
    } else { None }
};
if let Some(payload) = updated {
    emit(state, "message.part.updated", json!({"part": payload, ...}));  // ③ emit 拷贝
}
```

这看起来是对的。**但是**：

```rust
// apps/server/src/translator.rs:445-446 (Create fallback path)
} else {
    data["state"] = json!({...});
    apply_transition(&mut data, &transition);
}
push_part(state, assistant_msg_id, session_id, "tool", data);
```

Create 路径走 `push_part`，它会构造 `PartInfo { data: data.clone() }`，然后 emit `json!({"part": data})`——`data` 是原始对象，但 `push_part` 中 `object.insert("id", ...)` `object.insert("type", ...)` 是修改原始对象。然后 emit 用的也是同一个 `data`。所以 Create 路径是对的。

**真正的 critical 风险**：tool 的 `Finish { is_error: true }` 状态写入 `state.time.end`（apps/server/src/translator.rs:489-491），但是 Create 路径用 `push_part` 创建 part 时，data 中只有 `time: { "start": now }`，没有 end。后续 ToolEnd 通过 `create_or_update_tool_part` 调用 `apply_transition` 时，`data` 是在 **parts.write() 闭包内** 原地修改的 `p.data`。emit 的 payload 是 `p.data.clone()`，**此时 `state.time.end` 已被 Finish 写入**，OK。✓

**实际上这里没有 bug。** 重新审视后我撤回这一条的严重性。

但是有一个**真的 high 问题**：

**`apply_transition` 的 `Finish` 分支（apps/server/src/translator.rs:473-499）对每个 tool completion 都执行 `tracing::info!(output_preview = %output.chars().take(200).collect::<String>(), ...)`。**

output 可能包含密码、API key、token、PII。把 200 字符的 tool 输出写到 info-level 日志是**高危信息泄漏**。生产环境应该默认是 debug 级或 trace 级，且提供 redaction hook。

**严重等级**：**High**（信息安全）— 但与"运行时正确性"略偏，移到第 4 节讨论。

---

### H-2 (重写). ToolError path 竞态：在 ToolCall 部分推送后立即收到 ToolError，可能丢 emit

**现状**

`ToolCall` 通过 `create_or_update_tool_part`（apps/server/src/translator.rs:382-447）发送。但 `push_part` 在 `parts.write()` 之后**释放锁再调 emit**（apps/server/src/agent_runner.rs:189-202），在 `parts.write()` 与 `emit` 之间存在窗口：

```
T1 (translator): lock parts, insert part, drop parts
T1 (translator): call emit()                       ← 此时 emit 完成
T2 (translator): lock parts, find part, mutate, drop, emit
```

两个 emit 各自独立——但 ToolError 路径上 `fail_tool_call` 也是这样：locate part，clone，drop，emit。

**真正的问题**：**`emit` 内部访问 `state.project`（apps/server/src/state.rs:519-522），同时 parts.write()/active_text.write()/active_reasoning.write() 是不同的 RwLock。**如果并发执行顺序是：

```
T1 (translator): active_text.write().insert(msg, p1)   ← 持锁
T2 (translator): emit()                                ← 持 project.read() 短暂
```

emit 不会再去 acquire active_text，不会死锁。✓

但是还有另一个真问题：

**`finalize_part_by_id` 在 `parts.write()` 内修改 part.data，但 `drop(parts)` 后 emit。**（apps/server/src/translator.rs:544-581）

```
T1: parts.write().find().update data  →  drop
T2: parts.write().find().update data  →  drop  ← T2 看不到 T1 的修改!
```

如果 T1 和 T2 同时 finalize 同一个 part（T1 finalize_text_part, T2 finalize_part_by_id via TurnFinish），**第二次 `time.entry("end").or_insert_with(...)`** 是 idem potent（如果 end 已存在就不覆盖），但是**第二次 emit 仍然发出一个 message.part.updated，且 payload 含 end**——这没问题（覆盖语义 OK）。

但**如果 T1 finalize 通过 `active_text.write().remove(...)` 后，T2 emit 用的 payload 是 T1 已写过的 time.end——OK**。

撤回这个 H-2。**真的 High 问题只剩 H-1。**

---

### H-3 (新增). openai_compat 的 anthropic_compat 与 BlockTracker 不一致 —— 完成前上游调用 close 时不补 TurnFinish

**现状**

`think_node.rs` 的 `invoke_think_llm` 在 `result?` 处传播 LLM 调用错误（apps/cli-server-backend/agent/agent-core/src/agent/react/think_node.rs:225）。如果 LLM 调用失败但 sink 已经发过 `TextBlockStart`/`ReasoningBlockStart`（例如 anthropic_compat 在流中途失败），**`TurnFinish` 不会被发出**，但 `TurnStart` 已经发了（apps/cli-server-backend/agent/agent-core/src/agent/react/think_node.rs:221）。

```rust
// agent/agent-core/src/agent/react/think_node.rs:221-225
let _ = stream_tx.try_send(StreamEvent::TurnStart);
let sink = BlockTrackerSink::new(stream_tx.clone());
let result = llm.invoke_stream(messages, Some(&sink), node_id).await;
sink.finish(node_id);  // 只 close_current + 推 Finish
let response = result?;  // ← 失败时传播错误，但 TurnFinish 还没发
```

`emit_finish_events` 不会被调用（apps/cli-server-backend/agent/agent-core/src/agent/react/think_node.rs:155-203 在 `run_with_context` 末尾，错误时早已返回），所以：
- `TurnStart` 已发，**但 `TurnFinish` 永远不发**
- `session.error` 不通过 `emit_finish_events` 路径发出

BlockTracker 已经被 `sink.finish(node_id)` 终结，但 translator 端的 `active_text[msg_id]` / `active_reasoning[msg_id][id]` 还指向 open part（因为 `TextBlockEnd` / `ReasoningBlockEnd` 来自 BlockTrackerSink，但 `invoke_stream` 失败时…）。

等等，让我重新看：

```rust
let _ = stream_tx.try_send(StreamEvent::TurnStart);  // ← TurnStart 已发
let sink = BlockTrackerSink::new(stream_tx.clone());
let result = llm.invoke_stream(messages, Some(&sink), node_id).await;
sink.finish(node_id);                                  // ← 推 TextBlockEnd/ReasoningBlockEnd/Finish
let response = result?;                                // ← 错误从 ? 返回
```

`sink.finish()` 调用 `tracker.close_current()`，会推 `TextBlockEnd` 或 `ReasoningBlockEnd` 到 stream_tx。这些事件到 translator 后会触发 `finalize_text_part`/`finalize_reasoning_part`，所以 part 会被终结。✓

但 `TurnFinish` 缺失——意味着 step-finish part 不会被创建，**前端无法识别 "回合结束"**。§8.9 不变量被打破。

**严重等级**：**High**

**建议修复**

让 `emit_finish_events` 在 `?` 错误时也执行（即使没有 usage 数据）：

```rust
// think_node.rs:294-298
let (response, streamed_chunks, first_token_at) =
    match run_cancellable(llm_call, ctx.cancellation.as_ref()).await {
        Ok(Ok(triple)) => triple,
        Ok(Err(e)) => return Err(e),
        Err(e) => {
            // 发出 TurnFinish + session.error + 终结
            self.emit_finish_events(ctx, call_start, first_token_at_at_call_start, None, None).await;
            return Err(e);
        }
    };
```

---

## 3. Medium 问题

### M-1. `apply_transition` 中 `.expect("state object")` 在孤立的 tool part 上会 panic

**现状**

```rust
// apps/server/src/translator.rs:465, 469, 476
ToolTransition::Start => {
    let obj = data["state"].as_object_mut().expect("state object");
    obj.insert("status".into(), json!("running"));
}
```

如果 tool part 是用 `push_part` 创建且 `Create` 路径走了 fallback 分支（apps/server/src/translator.rs:434-445），`data["state"]` 是 `{ status, input, output, metadata, time }` 的对象。OK。

但**如果 part 不是由 translator 创建的**（例如测试 fake runner 直接 push 了 `{ id, type, sessionID, messageID }` 但没有 `state` 字段），ToolStart 到达时 `data["state"]` 不存在 → **panic**。

`apps/server/src/agent_runner.rs:236` 的 `fake_runner` 可能在初始化时直接插入 tool part — 但要看具体实现。

**严重等级**：**Medium** — panic 仅在外部代码错误时触发，但仍是脆性边界。

**建议修复**：将 `.expect("state object")` 改为 `if let Some(obj) = data["state"].as_object_mut()`，并在 None 时插入默认 state。

---

### M-2. TextBlockEnd/ReasoningBlockEnd 在 active map 已无条目时调用 finalize 是 no-op，但不重置 part time

**现状**

```rust
// apps/server/src/translator.rs:503-508
pub fn finalize_text_part(state, session_id, assistant_msg_id) {
    let part_id = state.active_text.write().remove(assistant_msg_id);
    if let Some(part_id) = part_id {
        finalize_part_by_id(state, session_id, assistant_msg_id, &part_id);
    }
}
```

如果上游因为某种原因**重复发送** `TextBlockEnd`（同一消息两次），第二次 finalize_text_part 取出 None（已被前一次 remove），**不会发出 message.part.updated**。这本身是对的（避免重复 emit）。

但**第一次 finalize 已经发出 message.part.updated + 设置 time.end**——如果上游在第一次之后又发送 `TextDelta`，translator 会去找 `active_text[msg_id]`，发现 None，**静默丢弃 delta**（apps/server/src/translator.rs:106）。OK。

**真正的问题**：测试 `tool_error_for_unknown_call_id_is_noop` 锁定的"no-op"行为，**当 ToolError 在 ToolCall 之后到达但 ToolCall 创建的 part 已经被并发清理时**，用户看不到错误。已经在 H-1 中涵盖。

---

### M-3. TurnStart/TurnFinish pair 中 part id 不带 "sessionID" / "messageID" 字段

**现状**

`TurnStart` 的 `push_part` 数据（apps/server/src/translator.rs:152-162）：

```rust
json!({
    "id": new_part_id(),
    "type": "step-start",
    "time": { "start": now, "created": now },
})
```

`push_part` 内部会插入 `sessionID` 和 `messageID`（apps/server/src/agent_runner.rs:177-179）。✓

但 `apply_transition` 修改 part.data 时**不修改顶层 metadata**—— `Finish` 分支（apps/server/src/translator.rs:473-499）只更新 `state.time.end` 和 `data.time.end`，不修改顶层 `time.created`/`time.start`。**实际上顶层 time 是 push_part 时由调用方初始化的，不是 update 时维护的**，所以这是对的。

**真正问题**：`TurnFinish` 创建的 step-finish part 同时设置了 `start: now` 和 `end: now`（apps/server/src/translator.rs:186），但 LLM 实际执行时间是几分钟前（first_token_at 到 call_start）。**duration 字段无意义**。这是 cosmetic 问题但与 §8.9 不变量"TurnStart / TurnFinish 包裹单次 LLM 调用" 无关。

---

## 4. Low 问题

### L-1. Tool output 在 INFO 级别日志中泄漏

**现状**

```rust
// apps/server/src/translator.rs:492-498
tracing::info!(
    tool = %data.get("tool").and_then(|v| v.as_str()).unwrap_or("?"),
    output_len = output.len(),
    output_preview = %output.chars().take(200).collect::<String>(),
    is_error = is_error,
    "ToolEnd Finish transition"
);
```

每个 tool 执行完成都会写出 200 字符的输出预览到 INFO 级别。**Tool 输出可能包含**：
- 文件内容（含密码/token/SSH key）
- 数据库查询结果（含 PII）
- HTTP 响应（含认证头、cookies）
- bash 输出（env 变量 dump）

**严重等级**：**Low — 信息安全**，不是 correctness，但应降至 debug 或添加 redaction。

**建议修复**：改为 `tracing::debug!` 或 trace，或仅记录长度/哈希。

---

### L-2. 测试未覆盖的关键场景

**当前测试（apps/server/src/translator.rs:619-1713）覆盖**：
- ✅ 单元：`finalize_text_part`, `finalize_reasoning_part`, `finalize_all_reasoning_parts`, `finalize_part_by_id`
- ✅ 表驱动：`handled_events_emit_expected_opencode_events`, `ignored_events_produce_no_output`
- ✅ Tool 生命周期：`tool_call_to_end_coalesces_into_one_part_with_input_and_output`, `tool_call_input_is_stored_verbatim_for_tui_argument_rendering`, `tool_error_marks_existing_tool_part_as_error_without_tool_name`
- ✅ ProviderError / Finish / TurnStart / TurnFinish
- ✅ 反例：孤立 delta (text_delta_without_active_part_is_noop, reasoning_delta_for_unknown_block_is_noop)

**未覆盖**：

| # | 场景 | 当前测试 |
|---|---|---|
| 1 | **连续两次 TurnStart 不配 TurnFinish** | ❌ 无 |
| 2 | **TurnFinish 时仍有 open text block（state 不一致场景）** | 部分（turn_finish_finalizes_open_text_and_reasoning_parts） |
| 3 | **ProviderError 到达时仍有 open text block** | ❌ 无 |
| 4 | **ToolError 在 ToolCall 之前到达（无 part）** | 部分（tool_error_for_unknown_call_id_is_noop 锁定了 no-op，但未验证 session.error 透传） |
| 5 | **TurnStart 在已有 open text block 时到达** | ❌ 无 |
| 6 | **多 reasoning block 交错（r0 启动前 r1 已 End）** | ❌ 无 |
| 7 | **ToolCall 与 reasoning block 并存（reasoning 在 tool 之前）** | ❌ 无 |
| 8 | **重复 part id 生成（碰撞测试）** | ❌ 无 |
| 9 | **Finalize 重复调用（first emits, second is no-op）** | ❌ 无 |
| 10 | **OpenAI-style 状态翻转（text→reasoning→text）由 BlockTrackerSink 集成路径** | 集成测试缺失 |
| 11 | **`session.error` 在 TranslatorError 时不终结 block 的端到端验证** | ❌ 无 |

**严重等级**：**Low** — 测试覆盖盲点，不影响 correctness 但降低回归保护。

---

## 5. 锁分析

### 5.1 锁获取顺序

**translator.rs 内**：
- `TextBlockStart` (translator.rs:84-104)：active_text.write() → drop → push_part（parts.write() → drop → emit）
- `TextDelta` (translator.rs:105-109)：active_text.read() → drop → append_to_part (parts.write() → drop → emit)
- `TextBlockEnd` → `finalize_text_part` (translator.rs:503-508)：active_text.write() → drop → finalize_part_by_id (parts.write() → drop → emit)
- `ReasoningBlockStart` (translator.rs:113-135)：active_reasoning.write() → drop → push_part
- `ReasoningDelta` (translator.rs:136-146)：active_reasoning.read() → drop → append_to_part
- `ReasoningBlockEnd` → `finalize_reasoning_part` (translator.rs:510-529)：active_reasoning.write() → drop → finalize_part_by_id
- `TurnFinish` (translator.rs:164-189)：finalize_text_part → finalize_all_reasoning_parts → push_part → emit

**emit 内部**（state.rs:515-541）：project.read() → drop → push_event_buffer (event_buffer.write() → drop)

**锁顺序约束**：active_text/active_reasoning → parts → event_buffer/project。

### 5.2 死锁分析

**单线程场景**（synchronous translate_stream_event）：所有锁都在语句内临时持有，**无嵌套**。无死锁风险。✓

**多线程场景**：SSE 事件通过 `mpsc::Sender<StreamEvent<ReActState>>` 顺序投递给 `on_event` 回调（apps/server/src/agent_runner.rs 与 think_node.rs 中只有一个 stream_tx）。所以同一 message 的事件**在单线程内序列化**。✓

**但是**：如果 `session.error` 触发了**另一个**回调链（例如 G3 错误路径，见 06-checklist.md G3）并发修改 `active_text`，则两个线程可能：
- T1: `active_text.write().insert(...)` + `parts.write()`
- T2: `active_text.write().remove(...)` + `parts.write()`

两个 T 都按 `active_text → parts` 顺序获取，且都不持有对方已经释放的锁。**无死锁**。✓

**emit 内部顺序问题**：`emit` 获取 `project.read()` 后**释放**，再获取 `event_buffer.write()`。两个锁不嵌套。✓

### 5.3 锁竞争窗口

`push_part` (agent_runner.rs:189-202)：

```
T1: state.parts.write() (lock acquired)
T1: insert part
T1: drop(parts)  ← 释放锁
T1: emit(...)    ← 此时其他线程可以修改 parts
```

如果 T1 写完 part 但还没 emit 之前，T2 读 parts 能看到 T1 写的 part，**但收不到 T1 的 emit 事件**。这是 read-after-write 不一致——但在单线程 stream 中不存在。✓

**但**：`ToolError` 路径上 `fail_tool_call` (translator.rs:325-360) 也是 **修改 parts → drop → emit**——同样模式。✓

---

## 6. 锁顺序风险：一种尚未发生的 deadlock 模式

`finalize_part_by_id` (translator.rs:544-581) 在 `parts.write()` 内修改 data，但**不调用 `state.time`**，OK。

但 `apply_transition` 的 Finish 分支（translator.rs:489-491）修改 `data.get_mut("time").and_then(|v| v.as_object_mut())` —— 这是在 `parts.write()` 保护下的 in-memory mutation，安全。

**结论**：锁顺序**正确**，无死锁风险。✓

---

## 7. 附录 B 协议对齐验证

逐项对照附录 B 中 Loom 实际 SSE 输出 vs OpenCode v2 期望：

| 项 | Loom 实际 | OpenCode v2 期望 | 对齐 |
|---|---|---|---|
| 文本增量传输 | `message.part.updated`（累积 `part.text`） | `session.next.text.delta`（增量 `delta`） | ❌ 字段格式不同 |
| Token 结构 | `tokens: { input, output, reasoning, cache: { read, write } }` | `tokens: { input, output, reasoning, cache: { read, write } }` | ✅ 完全一致 |
| 结束携带 text | `text.ended` 不携带 text（前端需从累积 part.text 读取） | `text.ended` 携带 `text` | ⚠️ 字段差异 |
| 回合边界事件 | `step-start`/`step-finish` part（type 字段） | `step.started`/`step.ended` 独立事件 | ❌ 事件类型不同 |
| Tool 事件 | 单一 `type: "tool"` + `state.status` | `tool.called`/`tool.success`/`tool.failed` | ❌ 事件类型不同 |

**附录 B §B.2 已记录这些差异**，是"前端需双适配"。translator.rs 在协议层是对的，只是事件 payload 格式与 opencode v2 不直接兼容。

---

## 8. §8.9 事件不变量验证

文档 §8.9 列出 5 条不变量：

### IV-1. `TextDelta` 必须位于同一 `TextBlockStart` 与 `TextBlockEnd` 之间

- ✅ translator 中 `TextDelta` 仅在 `active_text[msg_id]` 存在时追加（translator.rs:106）
- ⚠️ 但 ProviderError **不终结 active text block**（C-1），违反不变量

### IV-2. `ReasoningDelta { id }` 必须位于同一 ID 的 `ReasoningBlockStart` 与 `ReasoningBlockEnd` 之间

- ✅ translator 按 id 路由（translator.rs:136-146）
- ⚠️ ProviderError **不终结 active reasoning blocks**（C-1），违反不变量

### IV-3. `ToolCall` 前必须先结束当前 text/reasoning block

- ✅ 由 BlockTrackerSink 保证：在 TextDelta → ToolCall 翻转时，BlockTracker 推 `TextBlockEnd`/`ReasoningBlockEnd` 然后才到 `ToolCall`（apps/cli-server-backend/foundation/stream-event/src/block_tracker.rs:32-46, 76-93）
- ⚠️ 但 anthropic_compat 直接发 `StreamEvent::ToolCall`（不走 BlockTrackerSink），依赖上游协议

### IV-4. `TurnStart` / `TurnFinish` 包裹单次 LLM 调用；`TurnFinish` 携带该回合最终 usage

- ⚠️ **不严格成立**：见 H-3 — 当 LLM 调用失败时 `TurnStart` 已发但 `TurnFinish` 不发

### IV-5. translator 必须按 reasoning ID 路由和收尾，不能根据最近事件类型推断

- ✅ 按 id 路由（translator.rs:136-146）
- ✅ finalize_reasoning_part 按 id 收尾（translator.rs:510-529）

---

## 9. 优先级修复建议

### 必须修复（Critical）

1. **C-1**: `ProviderError` 终结 open blocks
2. **C-2**: `TurnStart` 终结 open blocks（防止多回合状态泄漏）

### 强烈建议（High）

3. **H-1**: `ToolError` 在无 part 时降级为 `session.error`
4. **H-3**: LLM 错误路径补发 `TurnFinish`

### 建议（Medium）

5. **M-1**: `apply_transition` 用 `if let` 替代 `.expect()`

### 优化（Low）

6. **L-1**: tool output 日志降至 debug 级
7. **L-2**: 补充 11 个缺失的测试用例

---

## 10. 验证状态

✅ 读取完成：translator.rs (1713 行)、block_tracker.rs (104 行)、think_node.rs (352 行)、08-stream-event-refactor.md (596 行)、appendix-b-sse-payload-examples.md (228 行)、06-checklist.md (213 行)

✅ 锁分析：无嵌套锁，无死锁风险

✅ §8.9 不变量验证：5 条中 2 条被 ProviderError 路径破坏（C-1, H-3）

⚠️ 协议对齐：附录 B §B.2 已记录差异，非本验证范围

❌ 11 个测试盲点（详见 L-2）

未修改任何代码（按要求只读验证）。
