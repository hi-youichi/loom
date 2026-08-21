# 37 - Session List（全局会话列表与归档）

> 状态：已实现（loom `apps/acp` v0.5+）
> 前端消费方：`packages/ui/src/stores/globalSessions.ts`（`useGlobalSessionsStore` 侧栏数据源）

## 背景

侧栏「全局会话列表」（跨目录 + 已归档视图）原走 Express → opencode 的 `/api/experimental/session` passthrough。本扩展将该数据面迁移到 Loom：session 元数据存储于 loom 自己的 `acp_sessions` 表（SQLite，per-principal 隔离），归档状态服务端持久化并全局广播。

## 方法

域：`session`（即 `_loomdesk.dev/session/*`）

### `list-global`

请求参数：

```json
{
  "archived": false,
  "directory": "C:\\repo",
  "cursor": "<opaque hex>",
  "limit": 200
}
```

- `archived`（默认 `false`）：`true` 查已归档，`false` 查活跃。
- `directory`（可选）：目录过滤。服务端在比较前剥离 `\\?\` verbatim 前缀并统一分隔符（解决 Windows verbatim 路径不匹配问题）；子目录级联仍由前端客户端过滤。
- `cursor`（可选）：opaque 游标（hex 编码的 `updated_at` RFC3339 字符串），语义为「严格早于该时间」。
- `limit`（默认 200，上限 1000）。

响应：

```json
{
  "sessions": [
    {
      "sessionId": "session-…",
      "cwd": "\\\\?\\C:\\repo",
      "title": "…",
      "createdAt": "2026-08-18T14:39:17Z",
      "updatedAt": "2026-08-18T15:00:53Z",
      "archivedAt": null
    }
  ],
  "nextCursor": "373230…",
  "hasMore": true
}
```

- 按 `COALESCE(updated_at, created_at)` 降序（最近活跃在前）。
- `nextCursor` 为 `null` 表示遍历结束。
- 数据源为 `acp_sessions` 表；核心方法 `session/list`（活跃目录引导用）**排除已归档行**。

### `archive`

请求参数（camelCase）：

```json
{ "sessionId": "session-…", "archived": true }
```

- `archived=true` 写入 `archived_at=now` 并 bump `updated_at`；`false` 置 `NULL`（取消归档）。

### `update`

更新 Loom Desk-owned session metadata 或标题。metadata 与 ACP session 生命周期字段分表存储，调用方必须拥有该 session。

请求：

```json
{
  "sessionId": "session-…",
  "metadata": { "loomdesk": { "goal": { "status": "active" } } },
  "title": "可选的新标题"
}
```

`metadata` 和 `title` 至少提供一个；metadata 必须是 JSON object。响应返回更新后的 `session` 与完整 `metadata`。成功后广播 `session.updated`，供其他客户端刷新。
- 仅 owner（principal）本人可操作；目标不存在或不属于该 principal 返回 not_found。

响应：

```json
{ "session": { …同 list-global 元素… } }
```

#### 副作用：全局事件

归档变更通过 `GlobalEventBus` 广播（跨连接同步）：

- topic：`session`
- 事件类型：`session.updated`
- `properties.info`：opencode 形状的 session（`id`/`title`/`directory`/`time.created|updated|archived`(ms)/…），前端 `event-reducer` 既有归档分支直接消费。

## 错误

| code | message | 场景 |
|---|---|---|
| -32602 | invalid_params | 参数解析失败 / sessionId 为空 / cursor 非法 |
| -32001 | capability_not_supported | 域未注册（旧二进制） |
| -32003 | not_found | archive/update 目标不存在或非本 principal 所有 |
| -32603 | internal_error | SQLite 失败等 |

## 存储 schema（`acp_sessions`）

新增列 `archived_at TEXT`（`ALTER TABLE` 自动迁移，见 `session_repository.rs::ensure_archived_at_column`）；session-owned metadata 存储在 `acp_session_data.metadata_json`，不会改变 ACP 生命周期列。

## 迁移说明

- opencode 历史数据（`opencode.db`）**不导入**：loom 无法回放其消息（格式不同），导入只能产生死条目；原库保留作冷存储。
- 前端 `listGlobalSessionPages` 采用 ACP-first 分发：runtime 在线走本扩展，离线回退 opencode SDK（纯 opencode 远程运行时兼容路径）。ACP 出错**不回退**（避免两个数据集来回翻转）。
- Express `/api/experimental/session` 路由暂保留（非 ACP 回退路径依赖），待纯 opencode 运行时下线后移除。

## 实现索引

- Rust：`apps/acp/src/extensions/session_list.rs`（handler）、`apps/acp/src/session_repository.rs`（archive + metadata）、`apps/acp/src/agent.rs`（owner-scoped session mutations）
- FE：`packages/ui/src/stores/globalSessions.ts`（ACP 分发）、`packages/ui/src/lib/acp/acp-api.ts`（`session.listGlobal` / `session.archive` / `session.update`）、`packages/ui/src/lib/acp/type-mapping.ts`（`acpGlobalSessionToOpenCodeSession`）、`packages/ui/src/lib/acp/acp-runtime-shared.ts`（`probeAcpRuntime`：不自举连接的探测）
