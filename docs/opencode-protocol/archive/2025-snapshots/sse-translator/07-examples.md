# 事件序列对比示例与数据流图

> 返回 [README.md](README.md)

## 4.1 OpenCode 事件序列

场景：用户发消息 → LLM 先推理 → 输出文本 → 调用工具 → 再推理 → 输出最终文本

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

## 4.2 Loom 修改后事件序列（同样的场景）

> 开发任务：E3-E8（block/delta arm）、E9-E10（turn arm）、G1（emit）

**当前（有问题的序列）：**

```
run_agent 开始:
  (无状态重置)

LLM Stream #1:
  Messages(thinking)     → last part ≠ reasoning → 新建 reasoning prt_a
  Messages(thinking)     → last part = reasoning → 追加到 prt_a
  Messages(text)         → last part ≠ text → 新建 text prt_b
  Messages(text)         → last part = text → 追加到 prt_b
  ToolCall               → 创建 tool part tool-call_xxx
  ToolStart              → 更新 tool part → running
  ToolEnd                → 更新 tool part → completed
```

```
LLM Stream #2（仍在同一个 assistant_msg_id 下）:
  Messages(thinking)     → last part ≠ reasoning → 新建 reasoning prt_c
  Messages(text)         → last part ≠ text → 新建 text prt_d

run 结束:
  close_open_text_parts  → 补盖 time.end
```

**改造后序列：**

```
TurnStart                         → 创建 step-start part
  ReasoningBlockStart { id: "r0", metadata }
  ReasoningDelta { id: "r0", content: "Let me", metadata }
  ReasoningDelta { id: "r0", content: " think", metadata }
  ReasoningBlockEnd { id: "r0", metadata }
  TextBlockStart { metadata }
  TextDelta { content: "Running", metadata }
  TextDelta { content: " ls", metadata }
  TextBlockEnd { metadata }
  ToolCall → ToolStart → ToolOutput → ToolEnd
TurnFinish { reason, usage }      → 创建 step-finish part

TurnStart                         → 第二轮 step-start part
  ReasoningBlockStart { id: "r1" }
  ReasoningDelta { id: "r1", content: "Now I see..." }
  ReasoningBlockEnd { id: "r1" }
  TextBlockStart
  TextDelta { content: "The result is..." }
  TextBlockEnd
TurnFinish { reason, usage }
```

---

## 五、数据流图

> 开发任务：E2（translate_stream_event match 重写）、E1（active_text / active_reasoning 状态）

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
     ┌────────▼────────┐ ┌─────▼─────┐ ┌───▼────────┐
     │ TextDelta /     │ │ ToolXxx   │ │  Finish /  │
     │ ReasoningDelta  │ │           │ │  Others    │
     └────────┬────────┘ └─────┬─────┘ └────────────┘
              │                │
     ┌────────▼────────────────▼───────┐
     │ translate_stream_event match   │
     │ active_text / active_reasoning │
     └────────┬────────────────────────┘
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
