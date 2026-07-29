# 错误处理与重试管线

> 返回 [README.md](README.md)

## 2.10 错误处理与重试管线

> 开发任务：E12（ToolError arm）、E13（ProviderError arm）、G3（错误路径 message.error + session.error）、X8（重试策略）

### OpenCode 处理

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

### Loom 修改方案

Loom 当前缺乏完整的错误处理管线。建议对齐：

```rust
// agent_runner.rs — run_agent() 中的错误处理（短期）

match run_llm_stream(...).await {
    Ok(_) => {
        // 正常结束
        finalize_text_part(&state, &sid, &assistant_msg_id);
        finalize_all_reasoning_parts(&state, &sid, &assistant_msg_id);
    }
    Err(e) => {
        // ── 等价于 halt() ──
        // 1. 收尾活跃 part（cleanup 的 text/reasoning 部分）
        finalize_text_part(&state, &sid, &assistant_msg_id);
        finalize_all_reasoning_parts(&state, &sid, &assistant_msg_id);

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
