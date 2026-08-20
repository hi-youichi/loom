# Background Review ACP 协议设计

> **状态**：Draft，待评审
> **日期**：2026-08-20
> **范围**：Loom ACP Background review 的 session metadata、查询方法、通知和结果契约
> **相关代码**：`apps/acp/src/review_runner.rs`、`apps/acp/src/agent.rs`、`apps/acp/src/stream_bridge.rs`
> **交互文档**：Loom Desk `openchamber-feat-dev/docs/design/background-review-interaction.md`
> **架构文档**：[background-review-design.md](./background-review-design.md)

---

## 1. 设计目标

协议需要支持：

- session list 展示最近一次 Background review 状态；
- review 运行中的 realtime 状态通知；
- 刷新、重连后的状态恢复；
- 结构化 action details 查询；
- 手动启动和取消；
- 与代码审查循环 `auto-review` 域保持边界。

协议不应要求前端解析 assistant 自然语言摘要来恢复状态。

## 2. 当前协议基础

现有实现已经通过两条 ACP 路径暴露完成态：

1. `session/list` 的 `_meta.review`；
2. `session/update` 中的 `session_info_update._meta.review`。

完成后还会发送一条人类可读的 `agent_message_chunk`，但它只适合作为 UI 摘要，不应作为结构化数据源。

相关实现：

- `apps/acp/src/review_runner.rs:242-285`
- `apps/acp/src/agent.rs:2059-2180`
- `apps/acp/src/protocol.rs:55-90`

## 3. `_meta.review` 契约

### 3.1 字段

```jsonc
{
  "status": "running | completed | skipped | failed",
  "trigger": "background | review-skill",
  "startedAt": "2026-08-20T05:32:00Z",
  "reviewedAt": "2026-08-20T05:32:04Z",
  "memoryUpdateCount": 2,
  "skillUpdateCount": 1,
  "durationMs": 4200,
  "reviewId": "review-uuid",
  "reason": null
}
```

字段语义：

| 字段 | 类型 | 说明 |
|---|---|---|
| `status` | string | 当前 review 的最终或运行状态 |
| `trigger` | string | 自动触发或 `/review-skill` 触发 |
| `startedAt` | string? | review 启动时间，ISO 8601 |
| `reviewedAt` | string? | review 完成时间，ISO 8601 |
| `memoryUpdateCount` | integer | Memory 成功写入数量 |
| `skillUpdateCount` | integer | Skill 成功写入数量 |
| `durationMs` | integer? | 执行耗时 |
| `reviewId` | string? | 本次 review 的稳定 ID |
| `reason` | string? | skipped / failed 的机器可读原因 |

Rust 内部结构可以继续使用 snake_case；ACP wire 层使用 camelCase。

### 3.2 运行态示例

```json
{
  "status": "running",
  "trigger": "background",
  "startedAt": "2026-08-20T05:32:00Z",
  "memoryUpdateCount": 0,
  "skillUpdateCount": 0,
  "reviewId": "review-uuid"
}
```

### 3.3 完成态示例

```json
{
  "status": "completed",
  "trigger": "background",
  "reviewedAt": "2026-08-20T05:32:04Z",
  "memoryUpdateCount": 2,
  "skillUpdateCount": 1,
  "durationMs": 4200,
  "reviewId": "review-uuid"
}
```

### 3.4 跳过和失败示例

```json
{
  "status": "skipped",
  "trigger": "background",
  "reason": "insufficient_content",
  "reviewId": "review-uuid"
}
```

```json
{
  "status": "failed",
  "trigger": "background",
  "reason": "llm_error",
  "reviewId": "review-uuid"
}
```

## 4. Realtime 通知

review 状态变化通过现有 `session/update` 通道发送：

```jsonc
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess-123",
    "sessionUpdate": {
      "sessionInfoUpdate": {
        "_meta": {
          "review": {
            "status": "running",
            "reviewId": "review-uuid"
          }
        }
      }
    }
  }
}
```

完成态可以继续发送 assistant 摘要，但客户端应以 `_meta.review` 为状态真源。

通知使用 best-effort 发送；丢失通知不能导致状态丢失，客户端需要在 session list、status 或 history 查询时重新收敛。

## 5. `_loomdesk.dev/review/*` 扩展域

第一阶段可以只实现 metadata；第二阶段增加独立 review 域：

| 方法 | 方向 | 用途 |
|---|---|---|
| `review/status` | request | 查询 session 当前 review 状态 |
| `review/history` | request | 查询 session 或全局 review 历史 |
| `review/details` | request | 查询某次 review 的 action 明细 |
| `review/start` | request | 手动启动，可指定 scope |
| `review/cancel` | request | 取消当前 review |
| `review/changed` | notification | review 状态或结果变化 |

### 5.1 `review/status`

```json
{
  "sessionId": "sess-123"
}
```

```jsonc
{
  "sessionId": "sess-123",
  "active": false,
  "latest": {
    "reviewId": "review-uuid",
    "status": "completed",
    "trigger": "background",
    "memoryUpdateCount": 2,
    "skillUpdateCount": 1,
    "reviewedAt": "2026-08-20T05:32:04Z"
  }
}
```

### 5.2 `review/start`

```json
{
  "sessionId": "sess-123",
  "scope": "all",
  "trigger": "manual"
}
```

`scope` 取值：`all`、`memory`、`skills`。重复启动同一 session 时应返回当前 active review，而不是创建并发任务。

### 5.3 `review/cancel`

```json
{
  "sessionId": "sess-123",
  "reviewId": "review-uuid"
}
```

取消只作用于 Background review；不应发送 `session/cancel`，也不应中断主 Agent prompt。

### 5.4 `review/details`

```json
{
  "sessionId": "sess-123",
  "reviewId": "review-uuid"
}
```

```jsonc
{
  "sessionId": "sess-123",
  "reviewId": "review-uuid",
  "actions": [
    {
      "kind": "memory_create",
      "target": "PROJECT.md",
      "summary": "ACP session 使用独立 runtime",
      "succeeded": true
    },
    {
      "kind": "skill_update",
      "target": "rust-cli-cross-layer-feedback",
      "summary": "更新错误传播说明",
      "succeeded": true
    }
  ]
}
```

`actions` 应来自结构化 `ReviewActionSummary`，不应要求客户端从 summary 文本推断 action 类型。

## 6. 错误和权限

- 所有 request 必须校验 session 是否存在以及当前 principal 是否有权访问。
- `review/start` 和 `review/cancel` 属于写操作，需要 `auto-review` 等价的 capability / server policy 检查，但建议使用独立的 `background-review` capability 名称。
- 重复 start 返回已有 `reviewId`，不创建并发任务。
- cancel 目标不存在时返回明确的 `not_found` 或 `already_completed`，不要静默成功。
- action details 超出持久化大小限制时返回结构化错误，不截断为不可解释的文本。

## 7. 与 `auto-review` 的边界

`_loomdesk.dev/auto-review/*` 继续描述代码审查循环，包含 reviewer / implementer 阶段、severity、inline comments 和 review session。

Background review 使用 `_loomdesk.dev/review/*`，只描述 Memory / Skill 整理。两者可以共享 notification router、鉴权框架和前端活动中心，但不共享结果类型。

## 8. 兼容性与迁移

1. 老客户端忽略未知 `_meta.review` 字段即可继续工作。
2. `review` metadata 新字段全部使用 optional/default 兼容旧 session。
3. 旧客户端只消费 `agent_message_chunk` 时，仍能看到完成摘要。
4. 新客户端优先读取 `_meta.review`；缺失时回退为“尚未整理”，不把缺失解释为失败。
5. 协议扩展 capability 必须允许客户端按能力降级：无 `review/details` 时只展示 count 和 summary。

## 9. 测试计划

| 测试 | 验证点 |
|---|---|
| serde 契约测试 | camelCase 字段和 optional/default 行为 |
| session/list 测试 | 缺失、running、completed、skipped、failed 均可解析 |
| notification 测试 | start 和 completion metadata 正确发送 |
| extension 单测 | 参数、权限、session binding 正确 |
| dedup 测试 | 同一 session 不产生并发 review |
| reconnect 测试 | 丢失 realtime 后可通过 status/history 恢复 |
| details 测试 | action 类型、目标和成功状态不丢失 |
| cancel 测试 | 取消 review 不影响主 Agent |
