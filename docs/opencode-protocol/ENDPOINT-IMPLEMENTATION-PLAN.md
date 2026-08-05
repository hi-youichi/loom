# OpenCode Endpoint → Loom 实现方案

创建日期：2025-08-19。基线：opencode `packages/protocol/src/groups/*.ts`（17 Group / 85+ 端点），Loom `apps/server/src/routes.rs`（200+ 路由）。

## 1. opencode 协议结构

opencode 使用 Effect `HttpApi` 声明式定义全部路由，由 17 个 Group 组成：

| Group | 文件 | 端点数 | 职责 |
|-------|------|--------|------|
| Health | `groups/health.ts` | 1 | 健康检查 |
| Location | `groups/location.ts` | 1 | 当前工作目录信息 |
| Agent | `groups/agent.ts` | 1 | 列出可用 agent |
| Model | `groups/model.ts` | 1 | 列出可用 model |
| Provider | `groups/provider.ts` | 2 | 列出/获取 provider |
| Command | `groups/command.ts` | 1 | 列出 slash command |
| Skill | `groups/skill.ts` | 1 | 列出 skill |
| Reference | `groups/reference.ts` | 1 | 列出 reference |
| Event | `groups/event.ts` | 1 | 全局 SSE 事件流 |
| FS | `groups/fs.ts` | 3 | 文件读/列/搜 |
| Credential | `groups/credential.ts` | 2 | 凭据更新/删除 |
| Session | `groups/session.ts` | 17 | 会话全生命周期 |
| Message | `groups/message.ts` | 1 | 会话消息列表 |
| Permission | `groups/permission.ts` | 7 | 权限请求 CRUD |
| Question | `groups/question.ts` | 4 | 用户提问 CRUD |
| Integration | `groups/integration.ts` | 7 | 外部集成连接 |
| PTY | `groups/pty.ts` | 7 | 终端生命周期 |
| ProjectCopy | `groups/project-copy.ts` | 3 | 项目拷贝（实验性） |

### 中间件

- **Authorization**（`middleware/authorization.ts`）：Bearer token 校验，返回 `UnauthorizedError`
- **SchemaError**（`middleware/schema-error.ts`）：请求 schema 校验，返回 `InvalidRequestError`
- **Location**（server 注入）：工作目录上下文
- **SessionLocation**（server 注入）：session 级工作目录隔离

### 通用响应封装

- `Location.response<T>`：`{ data: T, location: Location.Info }`
- 分页响应：`{ data: T[], cursor: { previous?: Cursor, next?: Cursor } }`
- 错误体：`{ error: { _tag: string, message?: string } }`

---

## 2. Loom 当前状态

`apps/server/src/routes.rs` 已注册约 200+ 路由（含 v1 裸路径和 v2 `/api/*` 别名）。状态分三类：

### 2.1 已实现（真实逻辑）

| Group | 端点 | Handler 文件 |
|-------|------|-------------|
| Health | `GET /api/health` | `handlers/health.rs` |
| Location | `GET /api/location` | `handlers/bootstrap.rs` |
| Agent | `GET /api/agent` | `handlers/bootstrap.rs` |
| Model | `GET /api/model` | `handlers/bootstrap.rs` |
| Provider | `GET /api/provider`, `GET /api/provider/:id` | `handlers/bootstrap.rs` |
| Command | `GET /api/command` | `handlers/bootstrap.rs` |
| Skill | `GET /api/skill` | `handlers/bootstrap.rs` |
| Reference | `GET /api/reference` | `handlers/bootstrap.rs` |
| Integration | `GET /api/integration` | `handlers/bootstrap.rs` |
| FS | `GET /api/fs/list`, `GET /api/fs/read/*`, `GET /api/fs/find` | `handlers/fs.rs` |
| Credential | `PATCH/DELETE /api/credential/:id` | `handlers/credential.rs` |
| Event (SSE) | `GET /api/event`, `GET /api/session/:id/event` | `sse.rs` |
| Session | list, create, get, patch, delete, prompt, prompt_async, agent(model switch), context, history, compact, interrupt, message(s) | `handlers/session.rs`, `handlers/messages.rs` |
| Permission | `GET /api/permission/request`, `POST /api/permission/:id/reply` | `handlers/permission.rs` |
| Question | `GET /api/question/request`, `POST /api/question/:id/reply` | `handlers/question.rs` |
| PTY | `GET /api/pty` (list only) | `handlers/mcp_pty_file.rs` |

### 2.2 Stub / 501（路由已注册，无真实逻辑）

| 端点 | 类型 | 当前行为 |
|------|------|---------|
| `POST /api/session/:id/wait` | 缺失 | 未注册 |
| `GET /api/session/active` | stub | `v2_compat::active_sessions` 返回硬编码 |
| `POST /api/session/:id/revert/stage` | 501 | `handlers/revert.rs` — checkpoint store 未连接 |
| `POST /api/session/:id/revert/clear` | 501 | 同上 |
| `POST /api/session/:id/revert/commit` | 501 | 同上 |
| `POST /api/session/:id/permission` | stub | `v2_compat::true_value` |
| `GET /api/session/:id/permission` | stub | `v2_compat::empty_object` |
| `GET /api/session/:id/permission/:requestID` | 缺失 | 未注册 |
| `POST /api/session/:id/permission/:requestID/reply` | 缺失 | 未注册 |
| `GET /api/session/:id/question` | 缺失 | 未注册 |
| `POST /api/session/:id/question/:requestID/reject` | stub | `v2_compat::true_value` |
| `GET /api/integration/:id` | stub | `v2_compat::true_value` |
| `POST /api/integration/:id/connect/key` | 缺失 | routes.rs 注释 TODO |
| `POST /api/integration/:id/connect/oauth` | 缺失 | routes.rs 注释 TODO |
| `GET /api/integration/attempt/:attemptID` | 缺失 | routes.rs 注释 TODO |
| `POST /api/integration/attempt/:attemptID/complete` | 缺失 | routes.rs 注释 TODO |
| `DELETE /api/integration/attempt/:attemptID` | 缺失 | routes.rs 注释 TODO |
| `POST /api/pty` (create) | 501 | `handlers/mcp_pty_file.rs` |
| `GET /api/pty/:id` | 501 | 同上 |
| `PUT /api/pty/:id` | 501 | 同上 |
| `DELETE /api/pty/:id` | 501 | 同上 |
| `POST /api/pty/:id/connect-token` | 501 | 同上 |
| `GET /api/pty/:id/connect` | 501 | 同上 |
| `POST /experimental/project/:id/copy` | 缺失 | routes.rs 注释 TODO |
| `DELETE /experimental/project/:id/copy` | 缺失 | routes.rs 注释 TODO |
| `POST /experimental/project/:id/copy/refresh` | 缺失 | routes.rs 注释 TODO |

---

## 3. 分期实施计划

### Phase 1 — Session 补齐（P0：前端核心路径）

**目标**：让 OpenChamber 前端的 session 管理功能完全可用。

| # | 端点 | Handler 文件 | 实现方案 |
|---|------|-------------|---------|
| 1.1 | `POST /api/session/:sessionID/wait` | `handlers/session.rs` | 新增 `wait` 函数。轮询 `SharedState` 中 session 的运行状态，阻塞至 idle 后返回 `NoContent`（204）。超时 30s 返回 `ServiceUnavailableError`（503） |
| 1.2 | `GET /api/session/active` | `handlers/v2_compat.rs` | 替换 `active_sessions` stub。遍历 `SharedState.sessions`，返回 `{ data: { [id]: { type: "running" } } }` |
| 1.3 | `POST /api/session/:sessionID/revert/stage` | `handlers/revert.rs` | 内存快照方案：stage 时克隆 session messages 列表快照到 `SharedState.revert_snapshots: HashMap<SessionID, RevertSnapshot>`。返回 `{ data: { messageID, files: [] } }` |
| 1.4 | `POST /api/session/:sessionID/revert/clear` | 同上 | 清除 `revert_snapshots` 中的快照，返回 204 |
| 1.5 | `POST /api/session/:sessionID/revert/commit` | 同上 | 从快照恢复 messages 列表（截断到 target messageID），返回 204 |

**验证**：
- `POST /api/session/:id/wait` 对 idle session 立即返回 204
- `POST /api/session/:id/wait` 对 running session 阻塞，完成后返回 204
- revert stage → clear → 204
- revert stage → commit → messages 回滚

---

### Phase 2 — Session-Scoped Permission & Question（P1：交互闭环）

**目标**：让 OpenChamber 前端的审批/提问流程在 session 上下文中可用。

| # | 端点 | Handler 文件 | 实现方案 |
|---|------|-------------|---------|
| 2.1 | `POST /api/session/:sessionID/permission` | `handlers/permission.rs` | 新增 `create_session_permission`。在 `SharedState.permission_requests` 中写入请求，关联 `session_id`。请求体：`{ id?, action, resources, save, metadata, source, agent? }`；响应：`{ data: { id, effect } }` |
| 2.2 | `GET /api/session/:sessionID/permission` | 同上 | 新增 `list_session_permissions`。从 `permission_requests` 过滤 `session_id == path.sessionID` |
| 2.3 | `GET /api/session/:sessionID/permission/:requestID` | 同上 | 新增 `get_session_permission`。按 `session_id + request_id` 查找 |
| 2.4 | `POST /api/session/:sessionID/permission/:requestID/reply` | 同上 | 新增 `reply_session_permission`。复用现有 `post_permission_reply` 逻辑，增加 session 校验 |
| 2.5 | `GET /api/session/:sessionID/question` | `handlers/question.rs` | 新增 `list_session_questions`。从 `SharedState.question_requests` 过滤 `session_id` |
| 2.6 | `POST /api/session/:sessionID/question/:requestID/reject` | 同上 | 新增 `reject_session_question`。复用现有 `post_question_reject`，增加 session 校验 |

**实现模式**：
- 现有 `PermissionRequest` / `QuestionRequest` 结构体增加 `session_id: Option<String>` 字段
- 全局端点（`/api/permission/request`）不受影响——继续返回所有 pending
- session-scoped 端点只是加了 `session_id` 过滤
- `routes.rs` 将 stub handler 替换为真实 handler，移除 `v2_compat` stub 路由

**验证**：
- 创建 permission → session list 包含该请求 → reply 后消失
- 创建 question → session list 包含该问题 → reject 后消失

---

### Phase 3 — Integration 连接流（P2：外部集成）

**目标**：支持通过 API key 或 OAuth 连接外部集成（如 GitHub、Slack）。

| # | 端点 | Handler 文件 | 实现方案 |
|---|------|-------------|---------|
| 3.1 | `GET /api/integration/:integrationID` | `handlers/integration.rs` | 在现有 list 基础上增加 get-by-id。遍历内置 catalog 匹配 ID |
| 3.2 | `POST /api/integration/:integrationID/connect/key` | 同上 | 请求体：`{ key, label? }`。调用 `handlers/credential.rs` 的 credential store 存储 API key。返回 204 |
| 3.3 | `POST /api/integration/:integrationID/connect/oauth` | 同上 | 请求体：`{ methodID, inputs, label? }`。生成 `IntegrationAttempt`（UUID），存入 `SharedState.integration_attempts`。返回 `{ data: { attemptID, redirectURL } }` |
| 3.4 | `GET /api/integration/attempt/:attemptID` | 同上 | 从 `integration_attempts` 读取状态。返回 `{ data: { status: "pending" \| "completed" \| "expired" } }` |
| 3.5 | `POST /api/integration/attempt/:attemptID/complete` | 同上 | 请求体：`{ code? }`。标记 attempt 完成，存储 token 到 credential store。返回 204 |
| 3.6 | `DELETE /api/integration/attempt/:attemptID` | 同上 | 从 `integration_attempts` 移除。返回 204 |

**新增类型**：
```rust
struct IntegrationAttempt {
    id: String,
    integration_id: String,
    status: AttemptStatus,  // Pending | Completed | Expired
    redirect_url: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}
```

**验证**：
- connect/key → credential list 包含新凭据
- connect/oauth → attempt 创建 → poll pending → complete → 204
- attempt cancel → 404 on subsequent poll

---

### Phase 4 — PTY 生命周期 + ProjectCopy（P3：高级功能）

**目标**：完整 PTY 终端支持和项目拷贝功能。

| # | 端点 | Handler 文件 | 实现方案 |
|---|------|-------------|---------|
| 4.1 | `POST /api/pty` (create) | `handlers/pty.rs`（新建） | 请求体：`Pty.CreateInput { cwd, cols, rows, env? }`。使用 `portable-pty` 创建 PTY 对，存入 `SharedState.ptys`。返回 `{ data: Pty.Info }` |
| 4.2 | `GET /api/pty/:ptyID` | 同上 | 从 `ptys` 读取状态 |
| 4.3 | `PUT /api/pty/:ptyID` | 同上 | 请求体：`Pty.UpdateInput { cols, rows }`。调用 `pty.resize()` |
| 4.4 | `DELETE /api/pty/:ptyID` | 同上 | 关闭 PTY，从 `ptys` 移除 |
| 4.5 | `POST /api/pty/:ptyID/connect-token` | 同上 | 生成一次性 token，存入 `SharedState.pty_tokens`。返回 `{ data: { token } }` |
| 4.6 | `GET /api/pty/:ptyID/connect` | 同上 | `axum::extract::ws::WebSocketUpgrade`。验证 token 后双向转发 PTY I/O |
| 4.7 | `POST /experimental/project/:projectID/copy` | `handlers/project_copy.rs`（新建） | 请求体：`{ source, destination }`。异步启动拷贝任务。返回 `{ data: Copy.Info }` |
| 4.8 | `DELETE /experimental/project/:projectID/copy` | 同上 | 取消进行中的拷贝 |
| 4.9 | `POST /experimental/project/:projectID/copy/refresh` | 同上 | 刷新拷贝状态 |

**依赖**：`portable-pty` crate（创建伪终端），`axum` WebSocket 支持。

**验证**：
- create → get → resize → connect (WS) → 数据双向通 → delete
- project copy create → poll status → complete

---

## 4. 横切关注点

### 4.1 错误格式对齐

opencode 协议错误体格式：
```json
{
  "error": {
    "_tag": "SessionNotFoundError",
    "message": "Session abc123 not found"
  }
}
```

Loom 需统一 `AppError` enum 并实现 `IntoResponse`：
```rust
enum AppError {
    SessionNotFound(String),
    MessageNotFound(String),
    Conflict(String),
    InvalidCursor(String),
    InvalidRequest(String),
    ServiceUnavailable(String),
    Unauthorized(String),
    Unknown(String),
}
```

每个 variant 映射到对应的 HTTP 状态码和 `_tag` 字符串。

### 4.2 分页 Cursor

Session list 使用 base64url 编码的 JSON cursor：
```rust
struct SessionsCursorPayload {
    directory: Option<String>,
    project: Option<String>,
    subpath: Option<String>,
    order: Option<String>,  // "asc" | "desc"
    anchor: String,         // session ID 作为锚点
}
```

encode: `base64url(json(payload))`，decode 反向。

### 4.3 SSE 事件格式

v2 session SSE 使用 `SessionEvent.Durable` schema：
```json
{
  "type": "session.next.prompt",
  "sessionId": "...",
  "aggregateSequence": 42,
  "data": { ... }
}
```

Loom `sse.rs` 已有 session event stream，需确认 payload 字段名和嵌套结构与 opencode schema 一致。

### 4.4 路由注册策略

- Phase 1-2 的端点：替换 `routes.rs` 中已有的 stub 路由
- Phase 3 的端点：移除 `routes.rs` 底部注释中的 TODO，注册真实路由
- Phase 4 的端点：同上
- 所有新路由同时注册 `/api/*` 前缀版本

---

## 5. 工作量估算

| Phase | 新增/修改文件 | 新增端点 | 复杂度 | 估时 |
|-------|-------------|---------|--------|------|
| 1 | 3 文件 | 5 端点 | 中 | 1-2 天 |
| 2 | 2 文件 | 6 端点 | 低 | 1 天 |
| 3 | 1 文件 | 6 端点 | 中 | 1-2 天 |
| 4 | 2 文件 + 新依赖 | 9 端点 | 高 | 3-5 天 |
| 横切 | 2-3 文件 | — | 中 | 1-2 天 |
| **合计** | | **26 端点** | | **5-10 天** |

---

## 6. 验收标准

每个端点必须满足：
1. 请求/响应 schema 与 opencode `groups/*.ts` 声明一致
2. 错误返回正确的 `_tag` 和 HTTP 状态码
3. 无 `v2_compat::true_value` / `empty_list` / `empty_object` stub
4. 无 honest 501（除非依赖的外部系统未就绪，需在本文档中注明原因）
5. 至少一个集成测试覆盖 happy path 和主要 error path
