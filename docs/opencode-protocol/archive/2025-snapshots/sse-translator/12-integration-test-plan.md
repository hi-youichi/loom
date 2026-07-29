# 集成测试方案：StreamEvent 重构

> 返回 [README.md](README.md)
> 开发任务：T1-T4（见 [06-checklist.md](06-checklist.md)）

## 测试分层

| 层级 | 范围 | 已有覆盖 | 缺口 |
|---|---|---|---|
| **L1 单元** | BlockTracker 状态机 | ❌ 无 | C1 已新建但无测试 |
| **L2 单元** | translator match arm | ✅ 22 个测试 | 已完整 |
| **L3 集成** | BlockTracker → StreamEvent → translator → SSE buffer | ❌ 无 | **本方案核心** |
| **L4 集成** | SSE 流 → HTTP /oneshot → 响应体 | ✅ protocol.rs | 需新增 stream 事件路径 |

## 测试文件

新增 `apps/server/tests/stream_integration.rs`。

依赖现有基础设施：
- `loom_server::state::{new_state, SharedState}` — 构造测试 state
- `loom_server::translator::translate_stream_event` — 直接调用 translator
- `loom_server::state::snapshot_replay` — 读取 SSE 事件缓冲
- `stream_event::{BlockTracker, StreamEvent, StreamMetadata, Usage}` — 构造事件

## 测试用例

### L3-1: BlockTracker 单元（T1 补充）

文件：`foundation/stream-event/src/block_tracker.rs` `#[cfg(test)]`

```rust
#[test]
fn text_only_stream_produces_correct_sequence() {
    // on_text_delta("Hello") → on_text_delta("World") → on_finish
    // 期望: [TextBlockStart, TextDelta("Hello"), TextDelta("World"),
    //        TextBlockEnd, Finish]
}

#[test]
fn reasoning_then_text_produces_block_boundaries() {
    // on_reasoning_delta("Think") → on_text_delta("Answer") → on_finish
    // 期望: [ReasoningBlockStart{id:"r0"}, ReasoningDelta("Think"),
    //        ReasoningBlockEnd, TextBlockStart, TextDelta("Answer"),
    //        TextBlockEnd, Finish]
}

#[test]
fn text_then_reasoning_then_text_produces_two_flips() {
    // on_text_delta("A") → on_reasoning_delta("B") → on_text_delta("C") → on_finish
    // 期望: [TextBlockStart, TextDelta("A"), TextBlockEnd,
    //        ReasoningBlockStart{r0}, ReasoningDelta("B"), ReasoningBlockEnd,
    //        TextBlockStart, TextDelta("C"), TextBlockEnd, Finish]
}

#[test]
fn empty_delta_still_maintains_block_state() {
    // on_text_delta("") → on_text_delta("Hi") → on_finish
    // 期望: 只有一个 TextBlockStart（第二个 on_text_delta 不产出 Start）
}

#[test]
fn on_finish_without_active_block_only_emits_finish() {
    // 不调任何 delta，直接 on_finish
    // 期望: [Finish]（无 BlockEnd）
}

#[test]
fn close_current_on_none_is_noop() {
    // active = None, 调 close_current
    // 期望: []
}
```

### L3-2: BlockTracker → translator 全链路

文件：`apps/server/tests/stream_integration.rs`

```rust
/// 驱动 BlockTracker 产出事件 → 逐条喂给 translator → 断言 SSE buffer
fn run_block_tracker_through_translator(
    deltas: Vec<(&str, bool)>,  // (content, is_reasoning)
) -> (SharedState, Vec<String>) {
    let state = new_state();
    let mut tracker = BlockTracker::<TestState>::new();
    let metadata = StreamMetadata { loom_node: "think".into(), namespace: None };

    // TurnStart
    translate_stream_event(&StreamEvent::TurnStart, "sess", "msg", &state);

    for (content, is_reasoning) in &deltas {
        let events = if *is_reasoning {
            tracker.on_reasoning_delta(content, &metadata)
        } else {
            tracker.on_text_delta(content, &metadata)
        };
        for ev in events {
            translate_stream_event(&ev, "sess", "msg", &state);
        }
    }

    // TurnFinish
    for ev in tracker.on_finish(&metadata) {
        translate_stream_event(&ev, "sess", "msg", &state);
    }
    translate_stream_event(
        &StreamEvent::TurnFinish {
            reason: "stop".into(),
            usage: Usage { prompt_tokens: 10, completion_tokens: 20, total_tokens: 30, cached_tokens: None },
        },
        "sess", "msg", &state,
    );

    let events = snapshot_replay(&state, None);
    let types: Vec<String> = events.iter().map(|e| e.payload.event_type.clone()).collect();
    (state, types)
}
```

| # | 用例 | 输入 | 断言 |
|---|---|---|---|
| I1 | 纯文本 | `[("Hello", false), ("World", false)]` | parts 有 1 text part，内容 "HelloWorld"，有 `time.start` + `time.end` |
| I2 | 推理 → 文本 | `[("Think", true), ("Answer", false)]` | parts 有 1 reasoning + 1 text，各自独立 part_id |
| I3 | 文本 → 推理 → 文本 | `[("A", false), ("B", true), ("C", false)]` | parts 有 2 text + 1 reasoning，顺序正确 |
| I4 | 空字符串 delta | `[("", false), ("Hi", false)]` | 只有 1 text part（不因空串拆分） |
| I5 | 多回合 | 两轮上述序列 | 每轮有 step-start + step-finish part |
| I6 | usage 字段 | TurnFinish usage | step-finish part 的 `tokens` 字段完整 |

### L3-3: 多回合 agent loop

```rust
#[test]
fn multi_turn_agent_loop_produces_step_boundaries() {
    let state = new_state();
    let metadata = StreamMetadata { loom_node: "think".into(), namespace: None };

    // ── Turn 1: reasoning → text → tool ──
    translate_stream_event(&StreamEvent::TurnStart, "sess", "msg", &state);

    let mut tracker = BlockTracker::<()>::new();
    for ev in tracker.on_reasoning_delta("Let me", &metadata) {
        translate_stream_event(&ev, "sess", "msg", &state);
    }
    for ev in tracker.on_text_delta("Running ls", &metadata) {
        translate_stream_event(&ev, "sess", "msg", &state);
    }
    for ev in tracker.on_finish(&metadata) {
        translate_stream_event(&ev, "sess", "msg", &state);
    }
    translate_stream_event(
        &StreamEvent::TurnFinish {
            reason: "tool_calls".into(),
            usage: Usage { prompt_tokens: 100, completion_tokens: 50, total_tokens: 150, cached_tokens: None },
        },
        "sess", "msg", &state,
    );

    // Tool execution
    translate_stream_event(
        &StreamEvent::ToolCall { call_id: Some("c1".into()), name: "bash".into(), arguments: json!({"cmd": "ls"}) },
        "sess", "msg", &state,
    );
    translate_stream_event(
        &StreamEvent::ToolEnd { call_id: Some("c1".into()), name: "bash".into(), result: "file.txt".into(), is_error: false, raw_result: Some("file.txt".into()) },
        "sess", "msg", &state,
    );

    // ── Turn 2: text only ──
    translate_stream_event(&StreamEvent::TurnStart, "sess", "msg", &state);

    let mut tracker2 = BlockTracker::<()>::new();
    for ev in tracker2.on_text_delta("Done", &metadata) {
        translate_stream_event(&ev, "sess", "msg", &state);
    }
    for ev in tracker2.on_finish(&metadata) {
        translate_stream_event(&ev, "sess", "msg", &state);
    }
    translate_stream_event(
        &StreamEvent::TurnFinish {
            reason: "stop".into(),
            usage: Usage { prompt_tokens: 120, completion_tokens: 30, total_tokens: 150, cached_tokens: None },
        },
        "sess", "msg", &state,
    );

    // ── 断言 ──
    let parts = state.parts.read();
    let list = parts.get("msg").expect("parts exist");

    // 步骤边界
    let step_starts: Vec<_> = list.iter().filter(|p| p.part_type == "step-start").collect();
    let step_finishes: Vec<_> = list.iter().filter(|p| p.part_type == "step-finish").collect();
    assert_eq!(step_starts.len(), 2, "two turns → two step-start parts");
    assert_eq!(step_finishes.len(), 2, "two turns → two step-finish parts");

    // 第一轮的 reasoning 和 text
    let reasoning_parts: Vec<_> = list.iter().filter(|p| p.part_type == "reasoning").collect();
    assert_eq!(reasoning_parts.len(), 1);
    assert_eq!(reasoning_parts[0].data["text"], "Let me");

    let text_parts: Vec<_> = list.iter().filter(|p| p.part_type == "text").collect();
    assert_eq!(text_parts.len(), 2); // Turn 1 + Turn 2
    assert_eq!(text_parts[0].data["text"], "Running ls");
    assert_eq!(text_parts[1].data["text"], "Done");

    // Tool part
    let tool_parts: Vec<_> = list.iter().filter(|p| p.part_type == "tool").collect();
    assert_eq!(tool_parts.len(), 1);
    assert_eq!(tool_parts[0].data["state"]["status"], "completed");

    // 所有 text/reasoning part 都有时间戳
    for p in list.iter() {
        if matches!(p.part_type.as_str(), "text" | "reasoning") {
            assert!(p.data["time"]["start"].as_i64().is_some(), "{} missing time.start", p.part_type);
            assert!(p.data["time"]["end"].as_i64().is_some(), "{} missing time.end", p.part_type);
        }
    }

    // 第二轮的 usage
    let finish2 = step_finishes[1];
    assert_eq!(finish2.data["tokens"]["prompt"], 120);
    assert_eq!(finish2.data["tokens"]["completion"], 30);
}
```

### L3-4: 错误路径

```rust
#[test]
fn provider_error_mid_stream_emits_session_error() {
    // TurnStart → TextBlockStart → TextDelta → ProviderError
    // 断言: session.error 事件存在，active_text 已清理（TurnFinish 会清理）
}

#[test]
fn tool_error_marks_tool_part() {
    // ToolCall → ToolError
    // 断言: tool part status = "error"，error 字段 = message
}

#[test]
fn orphan_delta_is_silently_dropped() {
    // 直接发 TextDelta（无前置 TextBlockStart）
    // 断言: parts 为空，SSE buffer 为空
}

#[test]
fn orphan_reasoning_delta_for_unknown_id_is_dropped() {
    // 直接发 ReasoningDelta { id: "unknown" }
    // 断言: parts 为空，SSE buffer 为空
}
```

### L3-5: 状态重置

```rust
#[test]
fn state_reset_between_runs_clears_active_maps() {
    // 第一轮：TextBlockStart → TextDelta（不 close）
    // 手动清除 active_text + active_reasoning
    // 第二轮：TextBlockStart → TextDelta
    // 断言: 第二轮的 TextDelta 追加到新 part，不是旧 part
}
```

### L4: HTTP 端到端

文件：`apps/server/tests/stream_integration.rs`（追加）

```rust
#[tokio::test]
async fn sse_stream_endpoint_emits_expected_events() {
    // 构造 router + state
    // POST /api/session/{id}/prompt 或等效端点
    // 断言 SSE 响应体包含 message.part.updated + step-start + step-finish
    // 验证事件顺序
}
```

> L4 需要 mock LlmProvider，复杂度较高，建议作为第二优先级。

## 实施顺序

| 优先级 | 文件 | 内容 | 任务 |
|---|---|---|---|
| P0 | `foundation/stream-event/src/block_tracker.rs` | L3-1: BlockTracker 单元测试（6 个） | C1 补充 |
| P0 | `apps/server/tests/stream_integration.rs`（新建） | L3-2: 全链路参数化测试（I1-I6） | T2/T3 |
| P1 | 同上 | L3-3: 多回合 agent loop | T2 |
| P1 | 同上 | L3-4: 错误路径（4 个） | T3 |
| P2 | 同上 | L3-5: 状态重置 | T2 |
| P2 | 同上 | L4: HTTP 端到端（需 mock provider） | T3 |
