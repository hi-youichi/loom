# Loom vs OpenCode 端点合规审计

**流水线**：`audit-endpoints`（Phase A：静态合规）
**日期**：2026-07-18 23:57
**范围**：431 个端点，覆盖 Loom 后端 + opencode v1/v2 SDK

## 概要

| 来源 | 端点数 | 说明 |
|---|---|---|
| **Loom 后端**（apps/server/src/routes.rs + handlers/） | 165 | 多行 `.route()` 计数；实际唯一端点约 80 个。87 条 loom-only（含 9 个 v2_compat 桩 + Loom 扩展）。 |
| **opencode v1 SDK**（packages/sdk/js/src/gen/） | 78 | 旧版 SDK；部分端点已被 v2 namespace 方法取代。 |
| **opencode v2 SDK**（packages/sdk/js/src/v2/gen/） | 188 | 当前 SDK；从 opencode OpenAPI spec 自动生成。 |

| 矩阵分类 | 数量 |
|---|---|
| 匹配（param-agnostic key） | 59 |
| Loom 缺失（合计） | 129 |
| - critical | 50 |
| - major | 42 |
| - minor | 37 |
| Loom-only | 87 |
| 仅 alias（/api/ pair 不完整） | 0 |

| 行为差异严重度 | 数量 |
|---|---|
| critical | 2 |
| major | 14 |
| minor | 3 |
| nit | 15 |

**顶层结论**：Loom 已实现 v1 协议 envelope（`{directory, payload:{type,properties}}`）
和 v2 envelope（`{directory, project?, workspace?, payload:{type,properties,id?}}`）——
但 v2 TUI 调用的绝大部分 session lifecycle、SSE-per-session 和 TUI-control 端点
**缺失或为桩**。Loom-only 集合主要由 v2_compat 占位符（`true_value()` /
`empty_list()` / `empty_object()`）和 Loom 扩展（git 操作、fs 写操作、integration
API）组成，这些在 opencode 中不存在。

## Critical 级别发现（必修）

### C1. v2 SSE session-scoped 事件永不匹配

`GET /api/session/:sessionID/event` 是 v2-only 端点（Session3.event）。Loom
读取 `event.payload.properties.sessionID` 来过滤（apps/server/src/sse.rs:163-170），
但 v2 SSE 事件**没有 `properties` 字段**——sessionID 在 `event.data.sessionID`
（扁平 envelope）中。结果：v2 rollout 后 session-scoped SSE 订阅者收不到任何事件。

**修复**：当请求命中 v2 通道时，按 `event.data.sessionID` 过滤。

### C2. `GET /api/skill` 返回空列表——TUI 无法加载 agent skills

Loom 在 apps/server/src/routes.rs:188 的 `v2_compat::empty_list()` 桩意味着
TUI 拿到 `[]` 而不是 `AppSkillsResponses: {200: Array<{name,description,location,content}>}`。
TUI 无法 bootstrap skills。

**修复**：实现 `handlers::bootstrap::get_api_skills()`，从 `~/.claude/skills`
或 `cwd/.claude/skills` 读取。

### C3. `POST /tui/submit-prompt` 是 `true_value()` 桩

Loom 注册了 `POST /tui/submit-prompt`（apps/server/src/routes.rs:779），但
`true_value()` 返回 no-op。TUI 期望 prompt 被真正提交。

**修复**：桥接到 `session.prompt`，或文档化为 TUI 专用侧通道。

### C4. 缺失 50 个 v2 session/lifecycle 端点（critical 缺失矩阵）

整个 `/api/session/:id/{permission,question,revert,message}` 子树、
`/api/session/active`、`/api/session/:id/{context,compact,wait,interrupt,agent,model}`，
以及 `/api/{agent,health,location,event}` 在 v2 SDK 中都存在，但 **Loom 完全缺失**。
示例：

- `GET /api/session/:_/history`——v2 /api/ session/lifecycle 端点
- `PUT /auth/:_`——核心端点
- `GET /api/session/active`——v2 /api/ session/lifecycle 端点
- `GET /api/location`——v2 /api/ session/lifecycle 端点
- `GET /api/session/:_/message/:_`——v2 /api/ session/lifecycle 端点
- `POST /session/:_/unrevert`——核心端点
- `GET /session/:_/children`——核心端点
- `GET /api/permission/request`——v2 /api/ session/lifecycle 端点
- `GET /api/session/:_/question`——v2 /api/ session/lifecycle 端点
- `GET /api/health`——v2 /api/ session/lifecycle 端点
- `GET /global/dispose`——SSE stream
- `POST /api/session/:_/wait`——v2 /api/ session/lifecycle 端点
- ... 还有 38 个（见 diff_matrix.json）

**修复**：在 `apps/server/src/handlers/session.rs` 和
`apps/server/src/handlers/bootstrap.rs` 下实现缺失的 handler。

## Major 级别发现

### M1. Auth 中间件与 opencode TUI 不兼容

Loom 在 apps/server/src/auth.rs:170-202 加了 Bearer/Basic auth 闸门，这是
opencode 从未有的。当 `LOOM_AUTH_TOKEN` 或 `OPENCODE_SERVER_PASSWORD` 被设置时，
每个请求都需要该 header——但 opencode TUI 不发送。

**修复**：要么 (a) 文档化为 opt-in 并确保 TUI 读取环境变量，要么 (b) 让 auth
默认关闭，仅在显式配置时启用。

### M2. `LocationQuery` 读取 deepObject 查询，不是 `x-opencode-directory` header

Loom 期望 `?location[directory]=..&location[workspace]=..`（apps/server/src/location.rs:87-93），
但 v2 SDK 发送 `x-opencode-directory` header。静默回退到 server cwd。

**修复**：在 `LocationQuery::from_request_parts` 中同时读取 `x-opencode-directory`
和 `x-opencode-workspace` headers。

### M3. `GET /session` 返回扁平数组，v2 SDK 期望 `Location.response` envelope

`session::list` 在 apps/server/src/handlers/session.rs:44-52 直接返回 `Vec<Session>`。
v2 SDK 期望 `{200: LocationResponse<Session[]>}` = `{location, data}`。

**修复**：包到 `location_response()` helper 中。

### M4. `/global/upgrade` 和 `/global/instance/update` 返回 501 NOT_IMPLEMENTED

Loom 的 global_bus.rs handler 对这些返回 `StatusCode::NOT_IMPLEMENTED`。v2 SDK
错误映射（`GlobalUpgradeErrors`、`GlobalInstanceUpdateErrors`）**没有 501 项**，
所以客户端反序列化崩溃。

**修复**：要么从 registry 删除路由，要么返回 200 + no-op body，要么扩展 v2 SDK
错误映射。

### M5. `GET /session/:id` 返回空 404（handler bug）

`session_not_found()` helper 存在于 session.rs:29-39，但第 55 行的
`get_session()` 没有调用它——发出空 404 body。v2 SDK 按 typed error 的 `name`
字段区分。

**修复**：在 `None` 分支使用 `session_not_found(&id)`。

### M6. TUI control 桩

五个 `POST /tui/open-*` 端点返回 `true_value()`（apps/server/src/routes.rs:785-788）：
help、sessions、themes、models。TUI 可能需要在这些调用上注册服务端状态。

**修复**：实现真正的副作用，或从 registry 删除。

### M7. `/api/{permission/policy,experimental/tool,integration}` 是 `empty_*()` 桩

apps/server/src/routes.rs:651-653（tool）、707-708（permission/policy）返回空。
TUI 显示默认 tool list / permission policy / integration list——无真实数据。

**修复**：接到实际 config + agent registry。

### M8. 缺失 major 端点（矩阵）

- `GET /pty/shells`——data 端点
- `GET /file/content`——data 端点
- `POST /project/git/init`——data 端点
- `GET /api/integration/attempt/:_`——v2 /api/ data 端点
- `GET /api/reference`——v2 /api/ data 端点
- `POST /mcp/:_/disconnect`——data 端点
- `POST /api/integration/attempt/:_/complete`——v2 /api/ data 端点
- `GET /project/:_/directories`——data 端点
- ... 还有 34 个（见 diff_matrix.json）

## Minor 级别发现

### N1. session error 的 `_tag` vs `name` discriminator 字段

Loom 的 `session_not_found` 返回 `{_tag: 'SessionNotFoundError', sessionID, message}`
（session.rs:29-39）。opencode 的 `SessionPromptErrors` union 按 `name` 区分。
实践中两者都能工作，但字段名不同。

**修复**：将 `_tag` 重命名为 `name`（或文档化差异）。

### N2. `POST /global/dispose` 返回 `{ok: true, shutdown: true}` 而不是裸 boolean

apps/server/src/handlers/global_bus.rs:42——`GlobalDisposeResponses` 是
`{200: boolean}`。SDK 可能将 truthy object 解析为 `'true'`，但渲染不同。

**修复**：直接返回 `true`（boolean）。

### N3. 37 个 minor 缺失端点（矩阵）

大多是 v2 SDK 定义但 Loom 没实现的 `/experimental/*` console/capability/resource
端点。影响低（实验路径），但会破坏 TUI experimental 菜单。

## Schema 深度差异

_注：本次流水线迭代中未运行 schema-diff 阶段。以下占位仅基于行为差异证据。_

### 行为差异阶段识别到的字段级关注点

| 端点 | 字段 | Loom | OC v2 | 严重度 |
|---|---|---|---|---|
| `GET /session` | 响应 shape | `Vec<Session>` 扁平 | `{200: LocationResponse<Session[]>}` | major |
| `GET /global/dispose` | 响应 body | `{ok: true, shutdown: true}` | `boolean` | minor |
| `POST /session/:id`（error） | discriminator | `_tag` 字段 | `name` 字段 | minor |
| `PUT /auth/:providerID` | 整个端点 | 缺失 | `Auth3` body | critical |
| `GET /api/skill` | 整个端点 | `empty_list()` 桩 | `Array<{name,description,location,content}>` | critical |
| `GET /api/agent` | schema | `get_api_agents()` 静态 | `Agent.Info` 按 model 动态 | major |

**建议**：重新运行 schema-diff agent，为 59 个匹配端点生成 `schema_diffs.json`，
按字段对比（Rust struct vs TS interface）。

## 行为差异

逐端点行为发现（共 34 个）——见 `behavior_diffs.json` 获取结构化数据。

### SSE envelope（5 项）

| 端点 | Loom | OC | 判定 |
|---|---|---|---|
| `GET /event` | v1 envelope `{directory, payload}` | v1 TUI 期望 `{directory, payload}` | MATCH |
| `GET /api/event` | v2 envelope（扁平 + 外层字段） | `Event.Payload` 扁平 shape | MATCH |
| `GET /api/session/:id/event` | 过滤 `payload.properties.sessionID` | sessionID 在 `data.sessionID`（v2 扁平） | **BUG** |
| `GET /api/event` | 先 emit `server.connected` | 期望先收 `server.connected` | MATCH |
| `GET /api/event` | 每 10s 心跳 + KeepAlive 注释 | 每 10s 心跳 | MATCH |

### Auth 中间件（2 项）

| 端点 | Loom | OC | 判定 |
|---|---|---|---|
| ALL | 可选 Bearer/Basic；两者未设置时为开发模式 | 无 auth | **INCOMPAT** |
| ALL | 401 + `{error: '...'}` | 无 401 路径 | minor |

### Directory header（2 项）

| 端点 | Loom | OC | 判定 |
|---|---|---|---|
| ALL | 读取 `?location[directory]=..` deepObject | SDK 发 `x-opencode-directory` header | **INCOMPAT** |
| `GET /api/location` | `{directory, workspaceID?, project}` | 同 shape | MATCH |

### 状态码（4 项）

| 端点 | Loom | OC | 判定 |
|---|---|---|---|
| `POST /global/upgrade` | 501 | 错误映射无 501 | **INCOMPAT** |
| `POST /global/instance/update` | 501 | 错误映射无 501 | **INCOMPAT** |
| `DELETE /session/:id` | 204 / 404 | 期望 204，404 不在映射中 | minor |
| `POST /global/dispose` | 200 `{ok, shutdown}` | `{200: boolean}` | minor |

### 错误语义（2 项）

| 端点 | Loom | OC | 判定 |
|---|---|---|---|
| `GET /session/:id` | 空 404 body | typed `SessionNotFoundError` | **BUG** |
| `POST /session/:id/prompt` | error 带 `_tag` | discriminated `name` 字段 | minor |

### 其他（16 项）——见 `behavior_diffs.json`

包括：TUI 桩（`/tui/open-help|sessions|themes|models`）、agent/skill/tool list
响应、content-type（二进制读取）、幂等性、限流。

## 端点矩阵（前 50 个匹配）

| 状态 | 方法 | 归一化路径 | Loom handler | OC v1 | OC v2 |
|---|---|---|---|---|---|
| matched | GET | `/vcs/diff/raw` | `GET /vcs/diff/raw` | n/a | `GET /vcs/diff/raw` |
| matched | GET | `/find/symbol` | `GET /find/symbol` | n/a | `GET /find/symbol` |
| matched | PUT | `/pty/:_` | `PUT /api/pty/:ptyID` | n/a | `PUT /pty/:ptyID` |
| matched | GET | `/provider` | `GET /api/provider` | n/a | `GET /provider` |
| matched | GET | `/experimental/console` | `GET /experimental/console` | n/a | `GET /experimental/console` |
| matched | GET | `/pty/:_` | `GET /api/pty/:ptyID` | n/a | `GET /pty/:ptyID` |
| matched | GET | `/experimental/console/orgs` | `GET /experimental/console/orgs` | n/a | `GET /experimental/console/orgs` |
| matched | GET | `/pty/:_/connect` | `GET /api/pty/:ptyID/connect` | n/a | `GET /pty/:ptyID/connect` |
| matched | POST | `/session/:_/command` | `POST /session/:id/command` | n/a | `POST /session/:sessionID/command` |
| matched | GET | `/permission` | `GET /permission` | n/a | `GET /permission` |
| matched | POST | `/session/:_/abort` | `POST /session/:id/abort` | n/a | `POST /session/:sessionID/abort` |
| matched | GET | `/project/current` | `GET /project/current` | n/a | `GET /project/current` |
| matched | GET | `/session/:_` | `GET /session/:id` | n/a | `GET /session/:sessionID` |
| matched | GET | `/vcs/diff` | `GET /vcs/diff` | n/a | `GET /vcs/diff` |
| matched | GET | `/file/status` | `GET /file/status` | n/a | `GET /file/status` |
| matched | POST | `/permission/:_/reply` | `POST /permission/:requestID/reply` | n/a | `POST /permission/:requestID/reply` |
| matched | POST | `/session/:_/shell` | `POST /session/:id/shell` | n/a | `POST /session/:sessionID/shell` |
| matched | GET | `/global/event` | `GET /global/event` | n/a | `GET /global/event` |
| matched | GET | `/session/:_/todo` | `GET /session/:id/todo` | n/a | `GET /session/:sessionID/todo` |
| matched | GET | `/skill` | `GET /api/skill` | n/a | `GET /skill` |
| matched | GET | `/session` | `GET /session` | n/a | `GET /session` |
| matched | POST | `/global/upgrade` | `POST /global/upgrade` | n/a | `POST /global/upgrade` |
| matched | GET | `/vcs` | `GET /vcs` | n/a | `GET /vcs` |
| matched | DELETE | `/session/:_` | `DELETE /session/:id` | n/a | `DELETE /session/:sessionID` |
| matched | GET | `/find/file` | `GET /find/file` | n/a | `GET /find/file` |
| matched | POST | `/session/:_/share` | `POST /session/:id/share` | n/a | `POST /session/:sessionID/share` |
| matched | POST | `/question/:_/reject` | `POST /question/:requestID/reject` | n/a | `POST /question/:requestID/reject` |
| matched | GET | `/pty` | `GET /api/pty` | n/a | `GET /pty` |
| matched | GET | `/mcp` | `GET /mcp` | n/a | `GET /mcp` |
| matched | GET | `/config` | `GET /api/config` | n/a | `GET /config` |
| matched | GET | `/session/:_/diff` | `GET /session/:id/diff` | n/a | `GET /session/:sessionID/diff` |
| matched | POST | `/pty/:_/connect-token` | `POST /api/pty/:ptyID/connect-token` | n/a | `POST /pty/:ptyID/connect-token` |
| matched | DELETE | `/pty/:_` | `DELETE /api/pty/:ptyID` | n/a | `DELETE /pty/:ptyID` |
| matched | GET | `/global/config` | `GET /global/config` | n/a | `GET /global/config` |
| matched | GET | `/experimental/resource` | `GET /experimental/resource` | n/a | `GET /experimental/resource` |
| matched | POST | `/session/:_/init` | `POST /session/:id/init` | n/a | `POST /session/:sessionID/init` |
| matched | POST | `/session/:_/fork` | `POST /session/:id/fork` | n/a | `POST /session/:sessionID/fork` |
| matched | PATCH | `/session/:_` | `PATCH /session/:id` | n/a | `PATCH /session/:sessionID` |
| matched | GET | `/experimental/capabilities` | `GET /experimental/capabilities` | n/a | `GET /experimental/capabilities` |
| matched | POST | `/session/:_/summarize` | `POST /session/:id/summarize` | n/a | `POST /session/:sessionID/summarize` |
| matched | POST | `/question/:_/reply` | `POST /question/:requestID/reply` | n/a | `POST /question/:requestID/reply` |
| matched | GET | `/agent` | `GET /api/agent` | n/a | `GET /agent` |
| matched | POST | `/pty` | `POST /api/pty` | n/a | `POST /pty` |
| matched | GET | `/command` | `GET /api/command` | n/a | `GET /command` |
| matched | PATCH | `/global/config` | `PATCH /global/config` | n/a | `PATCH /global/config` |
| matched | GET | `/file` | `GET /file` | n/a | `GET /file` |
| matched | GET | `/find` | `GET /find` | n/a | `GET /find` |
| matched | GET | `/project` | `GET /project` | n/a | `GET /project` |
| matched | GET | `/provider/auth` | `GET /provider/auth` | n/a | `GET /provider/auth` |
| matched | PATCH | `/config` | `PATCH /api/config` | n/a | `PATCH /config` |

_（完整表：见 `diff_matrix.json` matches[]。59 匹配，129 缺失，87 loom-only。）_

## 建议下一步

1. **修复 SSE session 过滤 bug（C1）**——让 `event_matches_session()` 在 v2 envelope
   下读 `event.data.sessionID`。5 分钟改动。能解封所有 v2 session-scoped 事件消费者。

2. **实现 `get_api_skills()`（C2）**——从磁盘读取 skills，返回
   `Array<{name, description, location, content}>`。解封 TUI skill bootstrap。

3. **决策 `/tui/submit-prompt` 处理方式（C3）**——要么桥接到 `session.prompt`，要么
   文档化为 TUI 专用侧通道。在下一次 TUI 集成前决策。

4. **实现 50 个 critical 缺失端点（C4）**——整个
   `/api/session/:id/{permission,question,revert,message,context,compact,wait,interrupt,agent,model}`
   子树 + `/api/{agent,health,location,event}`。估算：每个 1-2 天完整 schema 对齐。

5. **修复 auth 中间件默认值（M1）**——让 auth 默认关闭，除非在生产部署中显式
   设置 `LOOM_AUTH_TOKEN`。否则需要文档化 TUI 的环境变量要求。

6. **为 `LocationQuery` 添加 `x-opencode-directory` header 回退（M2）**——
   `LocationQuery::from_request_parts` 单行改动。对 v2 TUI 目录路由至关重要。

7. **将 `session.list` 和 `session.get` 包到 `location_response()`（M3, M5）**——
   两个都是 5 行 envelope 修复。

8. **删除 501 路由或扩展 v2 SDK 错误映射（M4）**——二选一；不要在生产 registry
   中留 501。

9. **替换 9 个 v2_compat 桩（`true_value()` / `empty_list()` / `empty_object()`）**——
   列表见报告末尾。GA 不应留桩绕过。

10. **运行 `schema-diff` 阶段**——为 59 个匹配端点的字段级对比产出
    `schema_diffs.json`。目前仍是占位。

## 工件清单

- `.loom/artifacts/endpoint-audit/loom_endpoints.json`——提取的 165 个 Loom 路由
- `.loom/artifacts/endpoint-audit/opencode_v1_endpoints.json`——78 个 opencode v1 SDK 方法
- `.loom/artifacts/endpoint-audit/opencode_v2_endpoints.json`——188 个 opencode v2 SDK 方法
- `.loom/artifacts/endpoint-audit/diff_matrix.json`——匹配/缺失/loom-only 矩阵
- `.loom/artifacts/endpoint-audit/behavior_diffs.json`——34 条行为发现
- `.loom/artifacts/endpoint-audit/synthesis_summary.json`——本合成总结
- `.loom/artifacts/endpoint-audit/_matches.json`——匹配端点扩展详情（123KB）
- `.loom/artifacts/endpoint-audit/_critical_matches.json`——19 条 critical-match 详情

_尚未生成_：`schema_diffs.json`（本次流水线迭代中 schema-diff 阶段未运行）。

## 附录 A：v2_compat 桩（loom-only）

以下 21 条路由已注册但返回写死的 `true_value()` / `empty_list()` / `empty_object()`
占位符：

- `GET /experimental/resource/:id`——v2_compat 桩
- `DELETE /experimental/resource/:id`——v2_compat 桩
- `POST /session/:id/revert/stage`——v2_compat 桩
- `POST /experimental/resource`——v2_compat 桩
- `GET /api/permission/saved`——v2_compat 桩
- `POST /control/next`——v2_compat 桩
- `GET /formatter/status`——v2_compat 桩
- `POST /tui/control/exit`——v2_compat 桩
- `GET /api/fs/list`——v2_compat 桩
- `POST /experimental/console/org`——v2_compat 桩
- `GET /experimental/resource/list`——v2_compat 桩
- `POST /global/instance/update`——v2_compat 桩
- `POST /experimental/eval`——v2_compat 桩
- `GET /lsp/status`——v2_compat 桩
- `PATCH /mcp`——v2_compat 桩
- `POST /session/:id/revert/clear`——v2_compat 桩
- `PATCH /api/global/event/:id`——v2_compat 桩
- `POST /tui/command`——v2_compat 桩
- `POST /tui/control/next`——v2_compat 桩
- `POST /tui/control/cancel/:request_id`——v2_compat 桩
- `POST /session/:id/revert/commit`——v2_compat 桩

## 附录 B：OC v1 端点（不在 OC v2 中）

78 个 v1 端点，大部分可能已被 v2 namespace 方法取代：

_为简洁省略——见 `opencode_v1_endpoints.json`_
