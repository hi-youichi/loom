# ACP Background Review 双通道通知方案

**创建时间**：2025-08-19  
**分支**：`acp/review-result-notify`  
**Commit**：`30b2bb80`

---

## 1. 背景与问题

Loom 的 background review 功能在每次 prompt 完成（或 `/review-skill` 命令）后，于独立 OS thread 中异步运行 curator review。Review 完成后：

- ✅ 已持久化到 SQLite（`ReviewHistory`）
- ✅ 已写日志（`tracing::info!`）
- ❌ **ACP 客户端完全不知道 review 发生了什么**

用户在 Zed / JetBrains 等 IDE 中看不到 review 结果——既没有 chat 流提示，也没有 session 列表状态更新。

## 2. 目标

让 background review 的完成状态**通过 ACP 协议传达给客户端**，且：

- 用户能在 chat 流**立即看到** review 总结
- IDE session 列表能获得结构化状态用于 badge 渲染
- 不污染正常 prompt turn 的对话流
- 协议合规，面向未来兼容

## 3. 方案探索

### 3.1 现状（改动前）

```
Prompt 完成
  └─ agent.rs:970 spawn_inprocess_review(thread_id, resolved, true, true, "background")
       └─ OS thread: run_review() → ReviewOutcome
            ├─ ReviewHistory::append()  → SQLite 持久化
            ├─ tracing::info!()          → 日志
            └─ （结束，无 ACP 通知）
```

`spawn_inprocess_review` 签名（改动前）：

```rust
// apps/acp/src/review_runner.rs
pub fn spawn_inprocess_review(
    thread_id: String,
    resolved: ResolvedModelConfig,
    review_memory: bool,
    review_skills: bool,
    trigger: String,
)
```

两处调用点（改动前）：

```rust
// apps/acp/src/agent.rs:818 — /review-skill 命令
crate::review_runner::spawn_inprocess_review(
    entry.thread_id.clone(),
    resolved,
    review_memory,
    review_skills,
    "review-skill".to_string(),
);

// apps/acp/src/agent.rs:970 — prompt 完成后台 review
crate::review_runner::spawn_inprocess_review(
    entry.thread_id.clone(),
    resolved_for_review,
    true,
    true,
    "background".to_string(),
);
```

ACP 通知通道（已有基础设施）：

```rust
// apps/acp/src/agent.rs:102
pub struct LoomAcpAgent {
    session_update_tx: Option<mpsc::Sender<SessionNotification>>,  // ← 关键通道
    ...
}

// apps/acp/src/stream_bridge.rs:505
pub struct SessionNotifier {
    tx: mpsc::Sender<SessionNotification>,
    session_id: SessionId,
}
// mpsc::Sender 是 Send + Clone，可安全传入 OS thread
```

### 3.2 ACP 协议约束（基于官方规范）

以下结论基于 ACP 官方文档（2026-06 stabilized）的完整 review：

| 协议机制 | 关键约束 | 来源 |
|----------|---------|------|
| `messageId` | "Clients MUST compare message IDs as opaque strings and MUST NOT parse or infer meaning from their structure." | [RFD: Message ID](https://agentclientprotocol.com/rfds/message-id) |
| `messageId` 变更 | "A change in `messageId` indicates a new message has started." | 同上 |
| `messageId` 生成者 | "Only the Agent generates protocol message IDs." | 同上 |
| `SessionInfoUpdate._meta` | "Agent provides custom metadata for client features (tags, status, etc.)" | [RFD: Session Info Update](https://agentclientprotocol.com/rfds/session-info-update) |
| `SessionInfoUpdate` 降级 | "Graceful degradation: Clients that don't handle this notification simply ignore it." | 同上 |
| Prompt Turn 边界 | `session/prompt` response (`stopReason`) 是 turn 的最后一条消息 | [Protocol: Prompt Turn](https://agentclientprotocol.com/protocol/v1/prompt-turn) |

### 3.3 Zed 客户端现状

基于 Zed 源码（`crates/acp_thread/src/acp_thread.rs`）和 GitHub issue 调查：

- Zed 处理 `SessionInfoUpdate` **只解析 `title` 字段**，`_meta` 被忽略
- [Zed Issue #57930](https://github.com/zed-industries/zed/issues/57930)：标题为 "Add native status and widget surfaces for external ACP agents"，明确指出当前 agents 必须把 status "collapse into visible messages"
- 结论：**纯 `_meta` 方案在 Zed 上今天 0 可见**

### 3.4 候选方案

#### 方案 A：AgentMessageChunk（发助手消息到 chat 流）

**思路**：Review 完成后，通过 `AgentMessageChunk` notification 发一条人类可读的总结消息到 chat 流。client 收到新的 `message_id` 后，按协议渲染为一条新的助手消息。

**改动点**：

1. **`review_runner.rs`** — 扩展 `spawn_inprocess_review` 签名 + 新增通知逻辑

```rust
// 签名新增两个参数
pub fn spawn_inprocess_review(
    thread_id: String,
    resolved: ResolvedModelConfig,
    review_memory: bool,
    review_skills: bool,
    trigger: String,
    tx: Option<mpsc::Sender<SessionNotification>>,  // 新增
    session_id: Option<SessionId>,                   // 新增
) {
    // ... review 逻辑不变 ...
    match result {
        Ok(outcome) => {
            // 新增：构建并发送 AgentMessageChunk
            let summary = format!(
                "Background review saved {} memories + {} skill ({:.1}s).",
                outcome.memory_count, outcome.skill_count,
                duration_ms as f64 / 1000.0
            );
            let msg_id = uuid::Uuid::new_v4().to_string();  // 协议要求：opaque string
            let chunk = ContentChunk::new(
                ContentBlock::Text(TextContent::new(summary))
            ).message_id(Some(MessageId::new(msg_id)));
            let notif = SessionNotification::new(
                session_id.clone(),
                SessionUpdate::AgentMessageChunk(chunk),
            );
            let _ = tx.try_send(notif);  // 非阻塞
        }
        Err(e) => { /* 同理发失败消息 */ }
    }
}
```

2. **`agent.rs`** — 两处调用点传入 `tx` + `session_id`

```rust
// agent.rs:818
crate::review_runner::spawn_inprocess_review(
    entry.thread_id.clone(),
    resolved,
    review_memory,
    review_skills,
    "review-skill".to_string(),
    self.session_update_tx.clone(),          // 新增
    Some(args.session_id.clone()),           // 新增
);

// agent.rs:970 同理
```

3. **`stream_bridge.rs`** — 无改动（AgentMessageChunk 已有支持）

| 维度 | 评估 |
|------|------|
| Zed 立即可见 | ✅ 新 `messageId` → 新消息气泡 |
| 协议合规 | ⚠️ Prompt Turn 边界灰色地带（turn 结束后发 update 未被明确禁止） |
| 改动量 | ~50 行 |
| 风险 | client 在 turn 结束后收到 chunk 的行为未知（可能丢弃/延迟/正常渲染） |

#### 方案 B：SessionInfoUpdate + `_meta.review`（结构化元数据）

**思路**：Review 完成后，发 `SessionInfoUpdate` notification，在 `_meta` 里携带结构化 review 状态。按 ACP 规范设计意图，这是"custom metadata for client features (tags, status)"的正确路径。

**改动点**：

1. **`review_runner.rs`** — 扩展签名 + 新增 meta 构建逻辑

```rust
pub fn spawn_inprocess_review(
    ...,
    tx: Option<mpsc::Sender<SessionNotification>>,
    session_id: Option<SessionId>,
) {
    // ... review 逻辑不变 ...
    let meta = build_review_meta(&outcome, duration_ms);
    // 构建结构化载荷
    let mut meta = Meta::new();
    meta.insert("review".to_string(), serde_json::json!({
        "status": "reviewed",
        "reviewed_at": chrono::Utc::now().to_rfc3339(),
        "memory_count": outcome.memory_count,
        "skill_count": outcome.skill_count,
        "duration_ms": duration_ms,
    }));
    let notif = SessionNotification::new(
        session_id.clone(),
        SessionUpdate::SessionInfoUpdate(
            SessionInfoUpdate::new().meta(meta)
        ),
    );
    let _ = tx.try_send(notif);
}
```

2. **`stream_bridge.rs`** — `StreamUpdate::SessionInfoUpdate` 变体需新增 `meta` 字段

```rust
// 改动前
SessionInfoUpdate { title: String }

// 改动后
SessionInfoUpdate { title: String, meta: Option<Meta> }
```

渲染逻辑也需改（`StreamUpdate → SessionUpdate` 转换处加 meta 分支）。

3. **`agent.rs`** — 两处调用点（同方案 A）

4. **`protocol.rs`** — 更新 `_meta.review` schema 注释

| 维度 | 评估 |
|------|------|
| Zed 立即可见 | ❌ Zed 忽略 `_meta`（只解析 title） |
| 协议合规 | ✅ 完全合规（RFD 原文就是为此设计的） |
| 改动量 | ~35 行 |
| 风险 | 今天 0 用户感知；等 Zed issue #57930 落地后才能生效 |

### 3.5 最终决策：双通道（A + B）

同时发两条通知，各司其职：

1. **`AgentMessageChunk`**：今天就让用户在 chat 流看到 review 结果
2. **`SessionInfoUpdate._meta.review`**：协议正确路径，等 Zed 升级（issue #57930）后自动生效

**理由**：
- A 解决"今天用户看不到"的迫切问题
- B 保证协议合规性和未来兼容性
- 两者互不干扰，各走独立的通知路径
- Zed 今天看到 A；Zed 明天升级后看到 B；两条路径永远并存
- `_meta.review` schema 字段在 `protocol.rs:86` 已声明，B 方案正好落地这个半成品

## 4. 详细实现

### 4.1 改动清单

| 文件 | 改动点 | 改动类型 | 行数 |
|------|--------|---------|------|
| `apps/acp/src/review_runner.rs` | `spawn_inprocess_review` 签名加 `tx` + `session_id` | 签名扩展 | +2 |
| `apps/acp/src/review_runner.rs` | review 完成（Ok）后调用 `notify_completion` | 新增调用 | +5 |
| `apps/acp/src/review_runner.rs` | review 失败（Err）后构造 synthetic `ReviewOutcome::skipped` 并通知 | 新增错误处理 | +8 |
| `apps/acp/src/review_runner.rs` | `notify_completion` 函数：双通道发送 | 新增函数 | +35 |
| `apps/acp/src/review_runner.rs` | `build_summary_line`：渲染人读文本 | 新增函数 | +25 |
| `apps/acp/src/review_runner.rs` | `build_review_meta`：构建 `_meta.review` payload | 新增函数 | +20 |
| `apps/acp/src/review_runner.rs` | 7 个 unit test | 新增测试 | +120 |
| `apps/acp/src/stream_bridge.rs` | `StreamUpdate::SessionInfoUpdate` 变体加 `meta: Option<Meta>` 字段 | 枚举扩展 | +3 |
| `apps/acp/src/stream_bridge.rs` | `StreamUpdate → SessionUpdate` 渲染加 meta 分支 | 渲染逻辑 | +5 |
| `apps/acp/src/stream_bridge.rs` | `extract_title_from_react_event` 传 `meta: None` | 既有调用适配 | +2 |
| `apps/acp/src/stream_bridge.rs` | `try_send_session_meta` 新方法 | 新增方法 | +15 |
| `apps/acp/src/stream_bridge.rs` | `try_send_session_meta` 加 `warn!` 错误日志 | 错误处理 | +5 |
| `apps/acp/src/agent.rs` | `/review-skill` 调用点传 `tx` + `session_id` | 调用适配 | +3 |
| `apps/acp/src/agent.rs` | prompt 完成调用点传 `tx` + `session_id` | 调用适配 | +3 |
| `apps/acp/src/protocol.rs` | `session/update` 段加双通道通知说明 | 文档 | +6 |
| `apps/acp/src/protocol.rs` | `_meta.review` 段补充实时投递路径说明 | 文档 | +1 |

### 4.2 数据流

```
Prompt 完成 (agent.rs)
  │
  ▼
spawn_inprocess_review(tx, session_id, ...)
  │
  ├── OS thread spawn ────────────────────────────────┐
  │                                                   │
  │   run_review() → ReviewOutcome                    │
  │                                                   │
  │   ├── persist to SQLite (ReviewHistory)           │
  │   │                                               │
  │   └── notify_completion(tx, session_id, outcome)  │
  │       │                                           │
  │       ├── ① AgentMessageChunk                     │
  │       │   (随机 UUID message_id)                   │
  │       │   → chat 流即时可见                        │
  │       │                                           │
  │       └── ② SessionInfoUpdate                     │
  │           (_meta.review payload)                  │
  │           → IDE badge / 未来兼容                  │
  │                                                   │
  └───────────────────────────────────────────────────┘
                                                      │
                                                      ▼
                                          mpsc::Sender<SessionNotification>
                                                      │
                                                      ▼
                                          stdio_loop → JSON-RPC → Client
```

### 4.3 改动详解

#### 4.3.1 `review_runner.rs` — `spawn_inprocess_review` 签名扩展

**Before**：
```rust
pub fn spawn_inprocess_review(
    thread_id: String,
    resolved: ResolvedModelConfig,
    review_memory: bool,
    review_skills: bool,
    trigger: String,
) {
```

**After**：
```rust
pub fn spawn_inprocess_review(
    thread_id: String,
    resolved: ResolvedModelConfig,
    review_memory: bool,
    review_skills: bool,
    trigger: String,
    tx: Option<mpsc::Sender<SessionNotification>>,  // 新增：ACP 通知通道
    session_id: Option<SessionId>,                   // 新增：目标 session
) {
```

`tx` / `session_id` 为 `None` 时通知路径整体 no-op，兼容非 ACP 嵌入（测试、CLI fallback）。

#### 4.3.2 `review_runner.rs` — review 完成后调用 `notify_completion`

**Before**（Ok 分支结尾）：
```rust
info!(
    thread_id = %thread_id,
    skipped = outcome.skipped,
    "ACP in-process review completed"
);
// （函数结束）
```

**After**：
```rust
info!(
    thread_id = %thread_id,
    skipped = outcome.skipped,
    "ACP in-process review completed"
);
// 新增：双通道通知
notify_completion(
    tx.as_ref(),
    session_id.as_ref(),
    &thread_id,
    Ok(&outcome),
    start.elapsed().as_millis() as u64,
);
```

Err 分支同理，构造 synthetic `ReviewOutcome::skipped(format!("llm_error: {}", e))` 后调用 `notify_completion`。

#### 4.3.3 `review_runner.rs` — `notify_completion` 函数

```rust
fn notify_completion(
    tx: Option<&mpsc::Sender<SessionNotification>>,
    session_id: Option<&SessionId>,
    thread_id: &str,
    outcome: Result<&ReviewOutcome, ()>,
    duration_ms: u64,
) {
    let (Some(tx), Some(session_id)) = (tx, session_id) else { return };
    let outcome = match outcome { Ok(o) => o, Err(()) => return };

    let notifier = SessionNotifier::new(tx.clone(), session_id.clone());

    // ① AgentMessageChunk → chat 流
    let summary_line = build_summary_line(outcome);
    let msg_id = uuid::Uuid::new_v4().to_string();
    let chunk = ContentChunk::new(
        ContentBlock::Text(TextContent::new(summary_line))
    ).message_id(Some(MessageId::new(msg_id)));
    let msg_notif = SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AgentMessageChunk(chunk),
    );
    if let Err(e) = tx.try_send(msg_notif) {
        warn!(thread_id = %thread_id, error = %e,
              "Failed to send review summary chunk");
    }

    // ② SessionInfoUpdate._meta.review → IDE badge
    let meta = build_review_meta(outcome, duration_ms);
    notifier.try_send_session_meta(meta);
}
```

两条都是 `try_send`（非阻塞），channel 满时 warn 日志，不 crash。

#### 4.3.4 `review_runner.rs` — `build_summary_line`

人类可读的一行总结，覆盖三种场景：

```rust
fn build_summary_line(outcome: &ReviewOutcome) -> String {
    let secs = outcome.duration_ms as f64 / 1000.0;
    if outcome.skipped {
        let reason = outcome.skip_reason.as_deref().unwrap_or("skipped");
        return format!("Background review skipped ({}).", reason);
    }
    let parts: Vec<String> = [
        (outcome.memory_count > 0).then(||
            if outcome.memory_count == 1 { "1 memory".into() }
            else { format!("{} memories", outcome.memory_count) }
        ),
        (outcome.skill_count > 0).then(||
            if outcome.skill_count == 1 { "1 skill".into() }
            else { format!("{} skills", outcome.skill_count) }
        ),
    ].into_iter().flatten().collect();
    if parts.is_empty() {
        return format!("Background review: nothing to save ({:.1}s).", secs);
    }
    format!("Background review saved {} ({:.1}s).", parts.join(" + "), secs)
}
```

| 场景 | 输出示例 |
|------|---------|
| reviewed + 有 actions | `Background review saved 2 memories + 1 skill (1.2s).` |
| reviewed + 无 actions | `Background review: nothing to save (0.5s).` |
| skipped | `Background review skipped (session_too_short).` |

#### 4.3.5 `review_runner.rs` — `build_review_meta`

构建 `_meta.review` 结构化载荷：

```rust
fn build_review_meta(outcome: &ReviewOutcome, duration_ms: u64) -> Meta {
    let mut meta = Meta::new();
    let status = if outcome.skipped { "skipped" } else { "reviewed" };
    let mut payload = serde_json::json!({
        "status": status,
        "reviewed_at": chrono::Utc::now().to_rfc3339(),
        "memory_count": outcome.memory_count,
        "skill_count": outcome.skill_count,
        "duration_ms": duration_ms,
    });
    if let Some(reason) = &outcome.skip_reason {
        payload.as_object_mut().unwrap()
            .insert("skip_reason".into(), reason.clone().into());
    }
    meta.insert("review".into(), payload);
    meta
}
```

```json
{
  "status": "reviewed" | "skipped",
  "reviewed_at": "2025-08-19T12:34:56Z",
  "memory_count": 2,
  "skill_count": 1,
  "duration_ms": 1234,
  "skip_reason": "session_too_short"  // 仅 skipped 时
}
```

#### 4.3.6 `stream_bridge.rs` — `SessionInfoUpdate` 枚举扩展

**Before**：
```rust
SessionInfoUpdate { title: String }
```

**After**：
```rust
SessionInfoUpdate {
    title: String,
    meta: Option<Meta>,  // 新增
}
```

渲染逻辑（`StreamUpdate → SessionUpdate` 转换）：

**Before**：
```rust
StreamUpdate::SessionInfoUpdate { title } => {
    SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title(title.clone()))
}
```

**After**：
```rust
StreamUpdate::SessionInfoUpdate { title, meta } => {
    let mut info = SessionInfoUpdate::new().title(title.clone());
    if let Some(m) = meta {
        info = info.meta(m.clone());
    }
    SessionUpdate::SessionInfoUpdate(info)
}
```

既有调用点适配（`extract_title_from_react_event`）：
```rust
// Before
.map(|title| StreamUpdate::SessionInfoUpdate { title: title.clone() })
// After
.map(|title| StreamUpdate::SessionInfoUpdate { title: title.clone(), meta: None })
```

#### 4.3.7 `stream_bridge.rs` — `try_send_session_meta` 新方法

```rust
/// Send a session metadata update with an `_meta` payload (no title change).
pub fn try_send_session_meta(&self, meta: Meta) {
    let mut info = SessionInfoUpdate::new();
    info = info.meta(meta);
    let notif = SessionNotification::new(
        self.session_id.clone(),
        SessionUpdate::SessionInfoUpdate(info),
    );
    if let Err(e) = self.tx.try_send(notif) {
        tracing::warn!(
            session_id = %self.session_id,
            error = %e,
            "Failed to send session info update with _meta payload"
        );
    }
}
```

#### 4.3.8 `agent.rs` — 两处调用点

**调用点 1**（`agent.rs:818` — `/review-skill` 命令）：

```rust
// Before
crate::review_runner::spawn_inprocess_review(
    entry.thread_id.clone(), resolved, review_memory, review_skills,
    "review-skill".to_string(),
);

// After
crate::review_runner::spawn_inprocess_review(
    entry.thread_id.clone(), resolved, review_memory, review_skills,
    "review-skill".to_string(),
    self.session_update_tx.clone(),          // 新增
    Some(args.session_id.clone()),           // 新增
);
```

**调用点 2**（`agent.rs:970` — prompt 完成后台 review）：

```rust
// Before
crate::review_runner::spawn_inprocess_review(
    entry.thread_id.clone(), resolved_for_review, true, true,
    "background".to_string(),
);

// After
crate::review_runner::spawn_inprocess_review(
    entry.thread_id.clone(), resolved_for_review, true, true,
    "background".to_string(),
    self.session_update_tx.clone(),          // 新增
    Some(session_id),                        // 新增（已在作用域内）
);
```

#### 4.3.9 `protocol.rs` — rustdoc 更新

`session/update` 段新增双通道通知说明（`protocol.rs:49-58`）：

```rust
//! - **Background review completion**: When `spawn_inprocess_review` finishes
//!   (post-prompt or `/review-skill`), two `session/update` notifications
//!   are emitted:
//!   1. `agent_message_chunk` — human-readable summary (...).
//!   2. `session_info_update` with `_meta.review = { status, reviewed_at,
//!      memory_count, skill_count, skip_reason?, duration_ms }` ...
```

`_meta.review` 段补充实时投递路径说明（`protocol.rs:86`）。

## 5. 测试覆盖

### 5.1 Unit Tests（7 个新增，全部通过）

| 测试名 | 覆盖场景 |
|--------|---------|
| `summary_line_for_reviewed_with_both_kinds` | reviewed + memory + skill |
| `summary_line_for_reviewed_with_no_actions` | reviewed + 无 actions |
| `summary_line_for_skipped` | skipped 路径 |
| `review_meta_marked_reviewed_when_actions_present` | `_meta.review` reviewed 字段 |
| `review_meta_marked_skipped_carries_reason` | `_meta.review` skipped + skip_reason |
| `notify_completion_emits_chunk_and_session_info_with_meta` | 双通道通知完整验证 |
| `notify_completion_noop_without_channel` | `None` fallback |

### 5.2 双通道测试细节

`notify_completion_emits_chunk_and_session_info_with_meta` 使用真实 `mpsc::channel`：

1. 创建 `mpsc::channel::<SessionNotification>(4)`
2. 调用 `notify_completion(Some(&tx), Some(&session_id), ...)`
3. `rx.recv().await` 验证第一条是 `AgentMessageChunk`，检查文本包含 "1 memory" / "2 skills"，`message_id.is_some()`
4. `rx.recv().await` 验证第二条是 `SessionInfoUpdate`，检查 `meta["review"]["status"] == "reviewed"`
5. `try_recv()` 确认 channel 已空

## 6. 协议合规性分析

### 6.1 AgentMessageChunk（通道 ①）

| 规范要求 | 本实现 | 合规 |
|---------|--------|------|
| `messageId` 是 opaque string | 随机 UUID，无结构语义 | ✅ |
| Agent 生成 `messageId` | review_runner 内 `uuid::Uuid::new_v4()` | ✅ |
| `messageId` 变更表示新消息 | 新 UUID = 新消息气泡 | ✅ |
| Prompt turn 边界 | turn 结束后发 update，协议未禁止但未明确允许 | ⚠️ 灰色 |

**灰色地带说明**：ACP Prompt Turn 规范描述了 `session/prompt` response (`stopReason`) 是 turn 的结束，但没有说 turn 结束后 agent 不能发 `session/update`。实际行为取决于 client 实现——需实测 Zed。

### 6.2 SessionInfoUpdate._meta（通道 ②）

| 规范要求 | 本实现 | 合规 |
|---------|--------|------|
| 用于 custom metadata (tags, status) | `_meta.review` = review status | ✅ |
| Client 可忽略 (_meta) | Zed 当前忽略，符合 "graceful degradation" | ✅ |
| 所有字段 optional | `status` + counts 都是推荐字段 | ✅ |

## 7. 已知限制与风险

| 项目 | 说明 | 缓解 |
|------|------|------|
| Zed 在 turn 后处理 chunk 的行为未知 | 可能渲染、可能丢弃、可能延迟到下一个 prompt | 需 dev 环境实测 |
| Zed 不解析 `_meta.review` | session 列表 badge 今天不显示 | 等 issue #57930 落地 |
| `try_send` 可能丢消息 | mpsc channel 满（buffer=32）时 `Err` | warn 日志；单次 review 只发 2 条通知，不会满 |
| review 失败时发 synthetic skipped | 用户看到 "skipped (llm_error: ...)" | 合理行为，用户需知道 review 跑了但失败了 |

## 8. 未来演进

### 8.1 IDE 端（Zed / JetBrains）

- 解析 `SessionInfoUpdate._meta.review` 字段
- Session list 行尾渲染 badge：`✓ reviewed` / `⚠ skipped` / `○ pending`
- 可选：chat 流里对 review 的 `AgentMessageChunk` 做特殊样式（icon + 背景色）

### 8.2 Review 结果注入下次 prompt

Review 完成后将 summary 写入 session 级 state，下次 user 发 prompt 时作为 `<previous_review_outcome>` system context 注入。不经过 ACP 通知，而是自然出现在对话上下文里。

## 9. 文件索引

| 文件 | 说明 |
|------|------|
| `apps/acp/src/review_runner.rs` | 核心：`spawn_inprocess_review` + `notify_completion` + `build_summary_line` + `build_review_meta` + tests |
| `apps/acp/src/stream_bridge.rs` | `StreamUpdate::SessionInfoUpdate` 扩展 + `try_send_session_meta` |
| `apps/acp/src/agent.rs:818` | 调用点 1：`/review-skill` 命令 |
| `apps/acp/src/agent.rs:970` | 调用点 2：prompt 完成后台 review |
| `apps/acp/src/protocol.rs:49-58` | 协议文档：双通道通知说明 |
| `apps/acp/src/protocol.rs:86` | 协议文档：`_meta.review` schema |

## 10. 验证状态

| 检查项 | 状态 |
|--------|------|
| `cargo test -p acp --lib review_runner` | ✅ 12/12 pass |
| `cargo clippy -p acp --all-targets -- -D warnings` | ✅ zero warnings |
| `cargo fmt -p acp --check` | ✅ exit=0 |
| Zed 实际渲染验证 | ⏳ 待 dev 环境实测 |

---

## 附录 A：ACP 协议参考

- [Prompt Turn](https://agentclientprotocol.com/protocol/v1/prompt-turn)
- [Message ID RFD](https://agentclientprotocol.com/rfds/message-id)
- [Session Info Update RFD](https://agentclientprotocol.com/rfds/session-info-update)
- [Meta Field Propagation](https://agentclientprotocol.com/rfds/meta-propagation)
- [Zed Issue #57930: Native status and widget surfaces](https://github.com/zed-industries/zed/issues/57930)
