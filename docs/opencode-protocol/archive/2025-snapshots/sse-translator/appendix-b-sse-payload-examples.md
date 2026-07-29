# 附录 B：SSE JSON Payload 样例对比

> 返回 [README.md](README.md)

## B.1 场景：单回合（reasoning → text → tool → text）

### Loom 改造后实际 SSE 输出

```
event: session.status
data: {"sessionID":"sess_1","status":{"type":"busy"}}

event: message.updated
data: {"sessionID":"sess_1","info":{"id":"msg_user","role":"user"}}

event: message.updated
data: {"sessionID":"sess_1","info":{"id":"msg_asst","role":"assistant"}}
```

```json
// TurnStart → step-start part
event: message.part.updated
data: {
  "sessionID": "sess_1",
  "part": {
    "id": "prt_step_001",
    "type": "step-start",
    "time": { "start": 1724000000000, "created": 1724000000000 }
  },
  "time": 1724000000000
}
```

```json
// ReasoningBlockStart → reasoning part (empty)
event: message.part.updated
data: {
  "sessionID": "sess_1",
  "part": {
    "id": "prt_a",
    "type": "reasoning",
    "text": "",
    "time": { "start": 1724000000100, "created": 1724000000100 },
    "metadata": { "loom_node": "think" }
  },
  "time": 1724000000100
}
```

```json
// ReasoningDelta → reasoning part (accumulated)
event: message.part.updated
data: {
  "sessionID": "sess_1",
  "part": {
    "id": "prt_a",
    "type": "reasoning",
    "text": "Let me think"
  },
  "time": 1724000000200
}
```

```json
// ReasoningBlockEnd → reasoning part (finalized)
event: message.part.updated
data: {
  "sessionID": "sess_1",
  "part": {
    "id": "prt_a",
    "type": "reasoning",
    "text": "Let me think",
    "time": { "start": 1724000000100, "end": 1724000000300, "created": 1724000000100, "completed": 1724000000300 }
  },
  "time": 1724000000300
}
```

```json
// TextBlockStart → text part (empty)
event: message.part.updated
data: {
  "sessionID": "sess_1",
  "part": {
    "id": "prt_b",
    "type": "text",
    "text": "",
    "time": { "start": 1724000000300, "created": 1724000000300 }
  },
  "time": 1724000000300
}
```

```json
// TextDelta ×2 → text part (accumulated)
event: message.part.updated
data: {
  "sessionID": "sess_1",
  "part": {
    "id": "prt_b",
    "type": "text",
    "text": "Running"
  },
  "time": 1724000000400
}
// ... second delta: "Running ls"
```

```json
// TextBlockEnd → text part (finalized)
event: message.part.updated
data: {
  "sessionID": "sess_1",
  "part": {
    "id": "prt_b",
    "type": "text",
    "text": "Running ls",
    "time": { "start": 1724000000300, "end": 1724000000500, "created": 1724000000300, "completed": 1724000000500 }
  },
  "time": 1724000000500
}
```

```json
// ToolCall → tool part
event: message.part.updated
data: {
  "sessionID": "sess_1",
  "part": {
    "id": "prt_tool_1",
    "type": "tool",
    "tool": "bash",
    "state": { "status": "pending" },
    "input": { "command": "ls" },
    "time": { "start": 1724000000500 }
  },
  "time": 1724000000500
}
```

```json
// ToolEnd → tool part (completed)
event: message.part.updated
data: {
  "sessionID": "sess_1",
  "part": {
    "id": "prt_tool_1",
    "type": "tool",
    "tool": "bash",
    "state": { "status": "completed" },
    "output": "file.txt",
    "time": { "start": 1724000000500, "end": 1724000000600 }
  },
  "time": 1724000000600
}
```

```json
// TurnFinish → step-finish part
event: message.part.updated
data: {
  "sessionID": "sess_1",
  "part": {
    "id": "prt_step_001",
    "type": "step-finish",
    "reason": "tool_calls",
    "tokens": {
      "prompt": 100,
      "completion": 50,
      "total": 150,
      "cached": 10
    },
    "time": { "start": 1724000000000, "end": 1724000000600, "created": 1724000000000, "completed": 1724000000600 }
  },
  "time": 1724000000600
}
```

### OpenCode v2 期望的 SSE 输出（对比）

```json
// session.next.step.started
{ "type": "session.next.step.started", "sessionID": "sess_1", "assistantMessageID": "msg_asst", "agent": "default", "model": "minimax-m3" }

// session.next.reasoning.started
{ "type": "session.next.reasoning.started", "sessionID": "sess_1", "assistantMessageID": "msg_asst", "reasoningID": "r0" }

// session.next.reasoning.delta
{ "type": "session.next.reasoning.delta", "sessionID": "sess_1", "reasoningID": "r0", "delta": "Let me think" }

// session.next.reasoning.ended
{ "type": "session.next.reasoning.ended", "sessionID": "sess_1", "reasoningID": "r0", "text": "Let me think" }

// session.next.text.started
{ "type": "session.next.text.started", "sessionID": "sess_1", "assistantMessageID": "msg_asst", "textID": "t0" }

// session.next.text.delta
{ "type": "session.next.text.delta", "sessionID": "sess_1", "textID": "t0", "delta": "Running ls" }

// session.next.text.ended
{ "type": "session.next.text.ended", "sessionID": "sess_1", "textID": "t0", "text": "Running ls" }

// session.next.tool.called
{ "type": "session.next.tool.called", "sessionID": "sess_1", "callID": "c1", "tool": "bash", "input": {"command":"ls"}, "provider": {"executed": true} }

// session.next.tool.success
{ "type": "session.next.tool.success", "sessionID": "sess_1", "callID": "c1", "structured": {}, "content": [], "provider": {"executed": true} }

// session.next.step.ended
{ "type": "session.next.step.ended", "sessionID": "sess_1", "finish": "tool_calls", "cost": 0.001, "tokens": {"input": 100, "output": 50, "reasoning": 30, "cache": {"read": 10, "write": 5}} }
```

## B.2 关键结构差异

| 方面 | Loom SSE | OpenCode v2 SSE | 影响 |
|---|---|---|---|
| 事件名 | `message.part.updated` | `session.next.*.started/delta/ended` | 前端需双适配 |
| 文本传输 | 累积 `part.text`（完整） | `delta` 增量 | 前端需不同累加策略 |
| 回合边界 | part type `step-start`/`step-finish` | 独立事件 `step.started`/`step.ended` | 前端需按 type 过滤 |
| Token 结构 | `tokens: { prompt, completion, total, cached }` | `tokens: { input, output, reasoning, cache: { read, write } }` | 字段名和结构不同 |
| 结束携带 | `TextBlockEnd` 不携带 `text` | `text.ended` 携带完整 `text` | 重放时需从 delta 重建 |
| Tool 事件 | 单一 `type: "tool"` + `state.status` | 独立事件 `tool.called`/`tool.success`/`tool.failed` | 前端需双适配 |
