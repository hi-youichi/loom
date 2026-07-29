# Event 协议与 PartID

> 返回 [README.md](README.md)

## 2.7 Event 协议：updatePart vs updatePartDelta

> 开发任务：X1（追加 message.part.delta 事件）

### OpenCode 处理

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

事件 schema 定义（OpenCode v1 兼容路径，`packages/schema/src/v1/session.ts`）；v2 路径使用 `session.next.*`（`schema/session-event.ts:197-271`）：

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

### Loom 当前代码

```rust
// translator.rs:438-446 — 当前：每次 delta 都发送完整 part
// 改造后：emit 入口迁移到 TextDelta / ReasoningDelta match arm
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

### Loom 修改方案

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

**改造后衔接**：删除 `translate_chunk`，delta 仅通过 `StreamEvent::TextDelta` / `StreamEvent::ReasoningDelta` 两个 match arm 触发 emit。默认继续发送 `message.part.updated`；若客户端支持，可由两个 arm 额外发送本节定义的 `message.part.delta`。

---

## 2.8 PartID 生成策略

> 开发任务：X2（PartID 改为单调递增）

### OpenCode 处理

```typescript
// packages/opencode/src/session/schema.ts
// PartID 基于 单调递增有序 ID（时间戳 hex + 随机 base62，共 26 字节）
export const PartID = Schema.String.check(Schema.isStartsWith("prt")).pipe(
  Schema.brand("PartID"),
  statics((s) => ({
    ascending: (id?: string) => s.make(Identifier.ascending("part", id)),
  })),
)

// Identifier.ascending("part") 生成格式：prt_<6字节hex时间戳 + 14字节随机base62>
// 不是纯数字，而是 ULID 风格的有序字符串（packages/opencode/src/id/id.ts）
```

### Loom 当前代码

```rust
// state.rs — new_part_id()
pub fn new_part_id() -> String {
    format!("prt_{}", uuid::Uuid::new_v4())
}
```

Loom 使用 `prt_<uuid>`（随机），OpenCode 使用 `prt_<hex时间戳+random_base62>`（ULID 风格单调递增）。

### Loom 修改方案

**不需要修改**。客户端按 part 列表顺序追加显示（不依赖 ID 排序），
两种 ID 格式对消费端透明：
- 新 part → push 到列表末尾
- 已有 part → 按 part_id 匹配替换

UUID 的无序性不影响正确性，只影响 parts 列表的显示顺序（但 Loom 用 push 追加到列表末尾，
不依赖 ID 排序）。如果需要严格对齐：

OpenCode 的 `Identifier.ascending("part")`（`packages/opencode/src/id/id.ts`）格式为：
`prt_` + **12 hex chars**（`timestamp * 0x1000 + counter`，6 字节）+ **14 base62 chars**（随机）。
共 26 字符，前半段单调递增，后半段随机。**不是标准 ULID**（ULID 用 Crockford Base32）。

Rust 对齐实现：

```rust
use rand::Rng;

static LAST_TS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn new_part_id() -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let prev = LAST_TS.swap(now, std::sync::atomic::Ordering::Relaxed);
    let seq = if now != prev { 1 } else {
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
    };
    let combined = (now as i128) * 0x1000 + seq as i128;

    // 6 bytes hex (12 chars) — timestamp * 4096 + counter
    let time_hex = format!("{:012x}", combined & 0xFFFFFFFFFFFF);

    // 14 chars base62 random
    let chars = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut rng = rand::thread_rng();
    let random: String = (0..14).map(|_| chars[rng.gen_range(0..62)] as char).collect();

    format!("prt_{}{}", time_hex, random)
}
```

---

## 2.9 step-start / step-finish 事件

> 开发任务：E9（TurnStart arm）、E10（TurnFinish arm）、G1（emit TurnStart/TurnFinish）、G3（错误路径）

### OpenCode 处理

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

### Loom 当前代码

Loom 不生成 `step-start` / `step-finish` part。
LLM 的每轮调用由 `translate_stream_event` 中的事件序列隐式表示。

但 Loom 已有的 `TaskStart`/`TaskEnd` 是 **graph node 粒度**，与 OpenCode 的 LLM 调用粒度不同：

| | OpenCode `step-start`/`step-finish` | Loom `TaskStart`/`TaskEnd` |
|---|---|---|
| 粒度 | 单次 LLM 调用 | graph node（含 tool dispatch） |
| 触发时机 | LLM stream 开始/结束 | node 开始/结束（含 tool 处理） |
| Usage 数据 | 打包在 `step-finish` 内 | 独立 `StreamEvent::Usage` |

因此不能直接用 `TaskStart`/`TaskEnd` 映射。正确的方案是在 StreamEvent 中新增
`TurnStart` / `TurnFinish` 变体（见 [08-stream-event-refactor.md](08-stream-event-refactor.md)），
由 LLM client 层在 stream 开始/结束时直接发出：

| Loom StreamEvent | 时机 | → part |
|---|---|---|
| `TurnStart` | LLM stream 开始 | `step-start` part |
| `TurnFinish { reason, usage }` | LLM stream 结束（立即，不含 tool dispatch） | `step-finish` part |

**当前** 事件序列（agent loop 一个完整回合）：
```
TaskStart("think")  ← step-start
  Messages(thinking) ...
  Messages(text) ...
  ToolCall { ... }
  Usage { prompt_tokens, completion_tokens, ... }
TaskEnd("think")    ← step-finish
TaskStart("act")
  ToolStart / ToolOutput / ToolEnd
TaskEnd("act")
TaskStart("think")  ← 第二轮 step-start
  ...
```

**改造后序列**：`TaskStart/TaskEnd` 仅保留 graph node 生命周期语义，LLM 回合改由 `TurnStart/TurnFinish` 包裹；`Messages(thinking)` 拆为 `ReasoningBlockStart → ReasoningDelta → ReasoningBlockEnd`，`Messages(text)` 拆为 `TextBlockStart → TextDelta → TextBlockEnd`，usage 折叠进 `TurnFinish { reason, usage }`。

### Loom 修改方案

以 `TurnStart` / `TurnFinish` 生成 step parts。`TaskStart` / `TaskEnd` 仅表示 graph node 生命周期，不再映射 step。

translator 新增两个 arm：

```rust
StreamEvent::TurnStart => {
    let part_id = new_part_id();
    let now_ms = now_ms();
    push_part(state, assistant_msg_id, session_id, "step-start", json!({
        "id": part_id,
        "type": "step-start",
        "time": { "start": now_ms, "created": now_ms },
    }));
    emit_part_updated(state, session_id, part_id);
}

StreamEvent::TurnFinish { reason, usage } => {
    finalize_all_reasoning_parts(state, session_id, assistant_msg_id);

    let part_id = new_part_id();
    let now_ms = now_ms();
    push_part(state, assistant_msg_id, session_id, "step-finish", json!({
        "id": part_id,
        "type": "step-finish",
        "reason": reason,
        "tokens": {
            "prompt": usage.prompt_tokens,
            "completion": usage.completion_tokens,
            "total": usage.total_tokens,
            "cached": usage.cached_tokens,
        },
        "time": { "start": now_ms, "end": now_ms, "created": now_ms },
    }));
    emit_part_updated(state, session_id, part_id);
}
```

`TurnStart`/`TurnFinish` 由 LLM client 层在 stream 开始/结束时发出（见
[08-stream-event-refactor.md](08-stream-event-refactor.md) 的 SSE 映射表），
不再依赖 `TaskStart`/`TaskEnd` 或独立 `Usage` 事件。

**未纳入本轮 translator 改造的部分**（由独立机制实现）：

| OpenCode step-finish 行为 | 依赖 | Loom 状态 |
|---|---|---|
| patch part（文件变更快照） | snapshot.track() + snapshot.patch() | 无 snapshot 机制 |
| summary fork（异步摘要） | summary.summarize() | 无 summary 机制 |
| compaction check（token 溢出） | isOverflow() + config.compaction | 压缩内核已有（`ContextWindowCheck`、`PruneNode`、`CompactNode`），但 agent loop 集成、流事件协议、手动 compact 端点均未实现，见 [10-compaction.md](10-compaction.md) §10.1 |
