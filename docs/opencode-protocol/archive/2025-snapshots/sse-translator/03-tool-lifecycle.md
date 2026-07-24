# Tool Part 生命周期

> 返回 [README.md](README.md)

## 2.6 Tool Part 生命周期

> 开发任务：E12（ToolError arm）、X3（tool 250ms 宽限）、X4（aborted tool 标记）、X5（failToolCall blocked）

### OpenCode 处理

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

### Loom 当前代码

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
