# Loom vs OpenChamber 端点覆盖审计

**日期**: 2026-07-24
**范围**: OpenChamber 前端 (`C:\Users\heycj\dev\openchamber-feat-dev`) 实际向后端发出的 HTTP 请求
↔ loom-server (`apps/server/`) 已注册的路由
**目标**: 找出 OpenChamber 真实使用但 loom 缺失 / 桩化 / 行为不一致的端点

> 与 `docs/opencode-protocol/audits/loom-vs-opencode-endpoints.md` 的区别:
> 那篇比的是 **OpenCode 协议规范**（按 OpenAPI 生成的全量 SDK 方法）；
> 这篇比的是 **OpenChamber 真实代码路径**（按 SDK 调用点 + `runtimeFetch` 静态扫描），
> 是前者的子集 + OpenChamber 私有扩展。

---

## 0. 路径前缀约定

OpenChamber 经 vite dev proxy / vscode bridge proxy / 自带 Bun web server 转发到 loom 时
**只剥第一个 `/api` 前缀**：

- **OpenCode SDK v2 路径裸奔**（无 `/api`）：`/session`、`/provider`、`/config`、`/event`、
  `/app/agents`、`/tool/ids` 等。
- **`runtimeFetch('/api/...')` 路径**：openchamber 自己的 Bun web server 剥一次 `/api` 再转发；
  loom 实际收到的是 `/fs/list`、`/config/settings`、`/session-activity` 等。

loom-server 路由表（`apps/server/src/routes.rs`）**同时注册 `/foo` 和 `/api/foo`**，所以
两边都对得上。下表统一以"loom 实际收到"的路径形态列出。

---

## 1. 主流程（OpenCode SDK v2 调用）

调用点：`packages/ui/src/lib/opencode/client.ts`（通过 `createOpencodeClient({baseUrl, fetch: runtimeFetch})`，
SDK 导入路径 `@opencode-ai/sdk/v2`，`vite.config.ts:64` 别名到 `node_modules/@opencode-ai/sdk/dist/v2/client.js`）。

| SDK 调用 | HTTP（loom 收到） | loom 实现 | 状态 |
|---|---|---|---|
| `client.session.list` | `GET /session` | `handlers/session.rs::list_sessions` | ✅ |
| `client.session.create` | `POST /session` | `session::create_session` | ✅ |
| `client.session.get` | `GET /session/:id` | `session::get_session` | ✅ |
| `client.session.update` | `PATCH /session/:id` | `session::patch_session` | ✅ |
| `client.session.delete` | `DELETE /session/:id` | `session::delete_session` | ✅ |
| `client.session.messages` | `GET /session/:id/messages` | `handlers/messages.rs::list_messages_v1` | ✅ |
| `client.session.todo` | `GET /session/:id/todo` | `v2_compat::empty_list()` | ⚠️ **stub** `[]` |
| `client.session.promptAsync` | `POST /session/:id/prompt_async` | `session::post_session_prompt_async` | ✅ |
| `client.session.command` | `POST /session/:id/command` | `session::post_session_command` | ✅ |
| `client.session.abort` | `POST /session/:id/abort` | `session::post_session_abort` | ✅ |
| `client.session.shell` | `POST /session/:id/shell` | `session::post_session_shell` | ✅ |
| `client.session.fork` | `POST /session/:id/fork` | `session::post_session_fork` | ✅ |
| `client.session.summarize` | `POST /session/:id/summarize` | `session::post_session_summarize` | ✅ |
| `client.session.status` | `GET /session/status` | `handlers/messages.rs::session_status` | ✅ |
| `client.session.revert` | `POST /session/:id/revert` | — | ❌ **未注册**（`routes.rs:881-883` 仅 `TODO(W2)` 注释，仅 `revert/{stage,clear,commit}` 三个子路径未注册，SDK 调的是单端点 `revert`） |
| `client.session.unrevert` | `POST /session/:id/unrevert` | — | ❌ **未注册** |
| `client.path.get` | `GET /path` | `handlers/bootstrap.rs::path` | ✅ |
| `client.project.current` | `GET /project/current` | `v2_compat::true_value()` | ⚠️ stub |
| `client.app.agents` | `GET /app/agents` | — | ❌ 没注册 |
| `client.app.agents` (fallback) | `GET /agent` | `bootstrap::agent` | ✅（`client.ts:1509` 已写 fallback） |
| `client.app.skills` | `GET /app/skills` | — | ❌ 没注册（loom 路径是 `/api/skill` 单数） |
| `client.config.update` | `PATCH /config` | `bootstrap::patch_config` | ✅ |
| `client.config.providers` | `GET /config/providers` | `bootstrap::get_config_providers` | ✅ |
| `client.command.list` | `GET /command` | `bootstrap::command_list` | ✅ |
| `client.file.read` | `GET /file?path=` | `mcp_pty_file::file_read` | ✅ |
| `client.file.list` | `GET /file?path=` | `mcp_pty_file::file_read` | ✅（同一 handler） |
| `client.tool.ids` | `GET /tool/ids` | `v2_compat::empty_list()` | ⚠️ **stub** `[]` |
| `client.permission.reply` | `POST /permission/:id/reply` | `permission::post_permission_reply` | ✅ |
| `client.permission.list` | `GET /permission` | `permission::list_pending_permissions` | ✅ |
| `client.question.reply` | `POST /question/:id/reply` | `question::post_question_reply` | ✅ |
| `client.question.reject` | `POST /question/:id/reject` | `question::post_question_reject` | ✅ |
| `client.question.list` | `GET /question` | `question::list_pending_questions` | ✅ |

---

## 2. OpenChamber 私有 `runtimeFetch('/api/...')` 调用

调用点：`packages/web/src/api/*.ts`、`packages/ui/src/stores/*.ts`。这些走 openchamber 自己的
Bun web server → 剥 `/api` → 转发到 loom。下方路径已经"剥后"形态。

| OpenChamber 路径（剥 `/api`） | loom 实际路径 | loom 实现 | 状态 |
|---|---|---|---|
| `GET /fs/list` | `GET /fs/list` | `fs::get_fs_list` | ✅ |
| `POST /fs/mkdir` | `POST /fs/mkdir` | `fs::post_fs_mkdir` | ✅ |
| `GET /fs/stat` | `GET /fs/stat` | `fs::get_fs_stat` | ✅ |
| `POST /fs/write` | `POST /fs/write` | `fs::post_fs_write` | ✅ |
| `POST /fs/delete` | `POST /fs/delete` | `fs::post_fs_delete` | ✅ |
| `POST /fs/rename` | `POST /fs/rename` | `fs::post_fs_rename` | ✅ |
| `GET /fs/read` (OpenChamber 用 `?path=`) | `GET /fs/read/*path` | `fs::get_fs_read` | ⚠️ **路径风格不一致**：loom 期望路径段（`/fs/read/foo/bar`），openchamber 发 `?path=` |
| `GET /fs/raw` (OpenChamber 用 `?path=`) | `GET /fs/read/*path` | `fs::get_fs_read` | ⚠️ 同上 |
| `POST /fs/reveal` | — | — | ❌ **未注册** |
| `GET /find/file` | `GET /find/file` | `mcp_pty_file::find_file` | ✅ |
| `POST /find` (content grep) | `POST /find` | `mcp_pty_file::find_text` | ✅ |
| `GET /find/symbol` | `GET /find/symbol` | `v2_compat::{data:[]}` | ⚠️ **stub**（LSP 未接） |
| `GET /config/settings` | `GET /config/settings` | `settings::get_settings` | ✅ |
| `PUT /config/settings` | `PUT /config/settings` | `settings::put_settings` | ✅ |
| `POST /config/reload` | `POST /config/reload` | `settings::post_reload` | ✅ |
| `GET /config/agents/:name` | `GET /config/agents/:name` | `v2_compat::empty_object()` | ⚠️ stub（`useAgentsStore.ts:272,362,433,492` 全调） |
| `PATCH /config/agents/:name` | `PATCH /config/agents/:name` | `v2_compat::empty_object()` | ⚠️ stub |
| `DELETE /config/agents/:name` | `DELETE /config/agents/:name` | `v2_compat::empty_object()` | ⚠️ stub |
| `GET /config/mcp` | `GET /config/mcp` | — | ❌ **路径不一致**：loom 用 `/api/mcp`，openchamber 用 `/api/config/mcp` |
| `GET /config/mcp/:name` | `GET /api/mcp/:name` | — | ❌ 同上 |
| `GET /config/skills/*` | — | — | ❌ **openchamber 自有技能注册表**（`useSkillsStore.ts`），loom 不在协议层支持 |
| `GET /config/snippets/*` | — | — | ❌ openchamber 自有 |
| `GET /config/plugins/*` | — | — | ❌ openchamber 自有 |
| `GET /config/commands/*` | — | — | ❌ openchamber 自有 |
| `GET /quota/:providerId` | — | — | ❌ openchamber 自有（`useQuotaStore.ts:186`） |
| `GET /projects/:id/icon*` | — | — | ❌ openchamber 自有 |
| `GET /session-activity` | — | — | ❌ vscode webview 自己 mock（`main.tsx:439`） |
| `POST /sessions/:id/{view,unview,message-sent}` | — | — | ❌ vscode webview mock |
| `GET /github/*` | — | — | ❌ openchamber 自有（GitHub OAuth + PR/Issue） |
| `POST /openchamber/update-check` | — | — | ❌ openchamber 自有 |
| `GET /opencode-resolution` | — | — | ❌ openchamber 自有（`sync-context.tsx:1653`） |
| `POST /permission-auto-accept/sessions/:id` | — | — | ❌ openchamber 自有（自动接受策略） |
| `GET /push/*` `GET /client-auth/*` `GET /dictation/*` | — | — | ❌ openchamber 自有 |

---

## 3. SSE / WebSocket 事件流

| OpenChamber 调用点 | loom 收到 | loom 实现 | 状态 |
|---|---|---|---|
| `runtimeFetch('/health')`（`bootstrap.ts:98`、`sync-context.tsx:1660`） | `GET /health` | — | ❌ **没注册**（loom 只有 `/api/health` 和 `/global/health`） |
| `EventSource('/api/openchamber/events')`（`openchamberEvents.ts:136`） | `/openchamber/events` | — | ❌ openchamber 自有推送 |
| `EventSource('/api/terminal/:id/stream')`（`terminalApi.ts:861`） | `/terminal/:id/stream` | — | ❌ **loom PTY 未接**（`/api/pty/*` 全 501/404/426） |
| `EventSource('/api/global/event')`（`runtime-url.ts:46`） | `GET /global/event` (SSE) | `sse::event_stream` (v1) | ✅ |
| `WebSocket('/api/global/event/ws')`（`event-pipeline.ts:212`） | `WS /global/event/ws` | — | ❌ **loom 没有 WS 端点，只有 SSE** |
| SDK `global.event()` async iterator（v2）→ `GET /api/event` | `GET /api/event` | `sse::api_event_stream` | ✅ |
| SDK `session.event()` async iterator（v2）→ `GET /api/session/:id/event` | `GET /api/session/:id/event` | `sse::api_session_event_stream` | ✅ |

---

## 4. 关键缺口（按优先级）

### P0 — 阻塞主流程

1. **`WS /global/event/ws`（事件主通道）**
   - openchamber 默认通过 WebSocket 订阅事件流（`event-pipeline.ts:212`）
   - loom 只暴露 SSE（`/api/event`、`/global/event`、`/api/session/:id/event`）
   - 修复方向二选一：
     - (a) loom 新增 WebSocket 端点，复用 `sse.rs` 的 event bus
     - (b) openchamber 改用 SSE（`EventSource` + 现有 relay 通道）

2. **`session.revert` / `session.unrevert`**
   - SDK 已调用（`client.ts:985, 1006`）
   - loom `routes.rs:856` 只有 `TODO(W2)` 注释，无路由
   - 修复：在 `handlers/session.rs` 实现，对应 `/api/session/:id/revert`、`/api/session/:id/unrevert`

3. **`fs.read` / `fs.raw` 路径风格**
   - loom: `GET /api/fs/read/<path-as-segments>`
   - openchamber: `GET /api/fs/read?path=<encoded>` （`files.ts:173`）
   - 修复：在 loom 的 `fs::get_fs_read` handler 兼容 `?path=` 查询参数

4. **`fs.reveal`**
   - openchamber 调（`files.ts:245`）
   - loom 没注册
   - 修复：在 `handlers/fs.rs` 加 `post_fs_reveal` 桩或真实现

### P1 — 影响功能但有 fallback

5. **`/health`（openchamber bootstrap 探测）**
   - openchamber bootstrap 调 `GET /health`（`bootstrap.ts:98`、`sync-context.tsx:1660`）
   - loom 只有 `/api/health` 和 `/global/health`
   - 修复：在 routes 加 `/health` 别名，或 openchamber 改 `/api/health`

6. **`app.agents` (v2 SDK)**
   - openchamber 已写 fallback（`client.ts:1509`），命中 `/agent`，✅
   - 但 `/app/agents` 仍未注册，调试时会有 404 日志噪音

7. **`app.skills` (v2 SDK)**
   - loom 只有 `/api/skill`（`bootstrap::get_api_skill`，单数 + bootstrap 形态）
   - openchamber 期望 `/app/skills` 数组 + `{name,description,location,content}` 形态
   - 修复：在 `handlers/bootstrap.rs` 加 `get_app_skills()`，扫描 `~/.claude/skills`、`cwd/.claude/skills`

8. **`config/agents/:name` 全 stub**
   - `useAgentsStore.ts:272,362,433,492` 实际 GET/PATCH/DELETE 调全
   - 修复：在 `handlers/bootstrap.rs` 读 `~/loom/agents.toml`，或至少返回合理 envelope

### P2 — 体验降级，不阻塞

9. **`session.todo`** — stub `[]`，openchamber 用作 UI 占位，可后续补
10. **`tool.ids`** — stub `[]`，openchamber 用作 tool registry 兜底
11. **`project.current`** — stub，openchamber 走 `path` + `project` 两步拼装可绕过
12. **`find/symbol`** — stub（LSP 未接），openchamber 走 content grep fallback
13. **`config/mcp*`** — 路径不一致（openchamber `/api/config/mcp` vs loom `/api/mcp`），临时可在中间层 rewrite
14. **`/api/terminal/:id/stream`** — PTY loom 全 501，需 `portable-pty` 接入
15. **PTY 整体** — `POST /api/pty` 501、`/api/pty/:id/connect` 426，`handlers/pty.rs` 待实现

### P3 — Loom-only（openchamber 不调，但保留）

- `/git/*`（branches/log/commit/stage/unstage）
- `/vcs/*`、`/vcs/diff/raw`（`text/x-diff`）
- `/api/fs/read/*path` 字节流（与上面 P0-3 路径风格问题重复，是同一处）
- `/tui/*`（约 17 个，TUI 控制面）
- `/acp` WebSocket JSON-RPC（`loom acp` 子进程桥接）
- `/auth`、`/instance`、`/location`、`/credential`、`/provider/:id/source`
- `/global/upgrade`、`/global/instance/update`（501）

---

## 5. 与 `loom-vs-opencode-endpoints.md` 的交叉引用

`docs/opencode-protocol/audits/loom-vs-opencode-endpoints.md` 已经审计了 loom vs 完整 OpenCode v2 SDK
（188 个方法）的差距。本篇是其子集验证：

| 类别 | opencode v2 SDK 缺失（`loom-vs-opencode-endpoints.md`） | openchamber 真实触及 | 差异说明 |
|---|---|---|---|
| Critical | 50 个（`/api/session/:id/{permission,question,revert,message,model,wait,...}`、`/api/session/active`、`/api/{health,location,event}` 等） | 实际触及：**revert**、**app.skills**、**/health**、**/api/permission/request** 子集 | openchamber 还没用上 v2 SDK 全部方法（多数是 TUI 用），所以本篇的 critical 数 ≤ 那篇 |
| Major | 14 个（M1-M14） | **fs.read 路径风格**（M2 同源）、**auth**（openchamber dev 默认无密码） | openchamber 的外部 host 模式避开了部分 auth 问题，但 `fs.read`/`fs.raw` 仍然是真阻塞 |

**结论**：本篇是 `loom-vs-opencode-endpoints.md` 的运行时验证 + openchamber 私有扩展
审计。**P0 的 4 项缺口（WS event、revert、fs.read 路径、fs.reveal）必须先解决**，否则
OpenChamber 主流程在 loom 后端下无法运行。

---

## 6. 验证步骤

```powershell
# 1. 启动 loom-server（外部 host 模式）
cd C:\Users\heycj\dev\worktrees\loom\cli-server-backend
cargo run -p loom-server -- serve --host 127.0.0.1 --port 18081

# 2. 启动 openchamber（指向 loom）
$env:OPENCODE_HOST       = "http://127.0.0.1:18081"
$env:OPENCODE_SKIP_START = "true"
cd C:\Users\heycj\dev\openchamber-feat-dev
bun run packages/web/dev

# 3. 探测 P0 端点
curl http://127.0.0.1:18081/global/health   # 应 {"healthy":true}
curl http://127.0.0.1:18081/health          # 应同上（loom 缺失 → 404 或重定向到 /global/health）
curl http://127.0.0.1:18081/session         # 应 []（无 session 时）
curl -X POST http://127.0.0.1:18081/session # 应创建并返回 Session
curl http://127.0.0.1:18081/app/skills      # 应 404 或 []（loom 缺失）
curl -X POST http://127.0.0.1:18081/session/test/revert  # 应 404（loom 缺失）
```

完整功能验收清单见 `docs/opencode-protocol/design/openchamber-verify-method.md`。
