# loom-server v2 协议接入设计

> 状态: ✅ 已完成（2026-07-13；25/25）
> 适用: `apps/server/` 增量开发，目标让 opencode TUI v1 与 v2 SDK 都能直连
> 相关技能: `loom-development`, `git-commit-workflow`

> **完成审计（2026-07-13）：** 25/25 项已完成。`cargo fmt --all -- --check`、`cargo check -p loom-server`、`cargo test -p loom-server`、`cargo clippy -p loom-server --all-targets -- -D warnings` 和 `scripts/check-protocol.ps1` 均通过。已在 opencode 客户端环境运行 `opencode attach http://127.0.0.1:18081`，视觉确认正常首页/Provider picker、无 fatal overlay；协议索引已同步。第 1-5 节保留实施前 baseline 记录，实际完成状态以第 6 节任务勾选、代码与门禁结果为准。

---

## 1. 背景

opencode TUI 已迁移到 `@opencode-ai/sdk/v2`（`C:\Users\heycj\dev\opencode\packages\tui\src\context\sdk.tsx:1`），bootstrap 不再调用 v1 的 `/project`、`/project/current`，改为 `sdk.client.v2.location.get`（`data.tsx:469`）和 `sdk.client.v2.{agent,command,model,provider,reference,skill,integration}.list`（`data.tsx:480-543`），错误以 `throwOnError: true` 触发硬失败。loom-server 当前仅实现 v1 路径 15 条（`apps/server/src/main.rs:34-58`），无法承载 v2 bootstrap，且缺少 v1 TUI 的部分关键路由（`/session/status`、`/provider/auth`、`/lsp/status`、`/formatter/status`、`/experimental/*`）。

**目标**:

- 在不破坏 v1 TUI 的前提下，loom-server 暴露 v2 协议子集
- 事件 envelope 同时满足 `GlobalEvent`（v1）与 `GlobalEventSchema`（v2）
- 补齐 v1 TUI 已请求但 loom-server 缺失的 bootstrap 路由
- 实现认证头兼容 + SSE 心跳 + 10s 节奏

终态: TUI 启动到首条 prompt 不再因 4xx 抛出渲染异常；v1/v2 TUI 共用同一服务器；事件广播统一由一条 `AppState::event_tx` 驱动。

---

## 2. 协议差异总览

| 维度 | v1 | v2 |
| --- | --- | --- |
| Bootstrap 入口 | `/project`、`/project/current` | `/api/location` |
| 模型/Agent 列表 | `/provider`、`/agent` | `/api/provider`、`/api/agent` |
| Provider 详情 | `/config/providers` | `/config/providers`（同路径） |
| 消息/Part | `/session/:id/messages` | `/api/session/{sessionID}/message` |
| 提交 prompt | `/session/:id/prompt` | `/api/session/{sessionID}/prompt` |
| 中止 | `POST /session/:id/abort` | `POST /api/session/{sessionID}/interrupt` |
| SSE | `GET /global/event` | `GET /api/event` |
| Health | `GET /global/health` | `GET /api/health` |
| Envelope 字段 | `directory + payload` | `directory + project? + workspace? + payload` |
| 事件命名 | `message.part.delta` 等扁平 | 同名 + `payload.properties` 嵌套为 v2 schema |
| 权限 | 单 `/permission`、`/permission/:id/reply` | `/api/permission`（含 `saved`、`request`、`{id}/reply`） |
| Question | 单 `/question/{id}/reply\|reject` | `/api/question/{requestID}/reply\|reject`、新增 `/api/question/request` |
| VCS | `/vcs` | `/api/vcs`、`/api/vcs/{status,diff,diff/raw}`、`/api/vcs/apply` |
| LSP/Formatter | `/lsp`、`/formatter` | `/api/lsp`、`/api/lsp/status`、`/api/formatter`、`/api/formatter/status` |
| File/Find | `/file*`、`/find*` | `/api/file*`、`/api/find*` |
| Resource | `/experimental/resource` | `/api/reference` |
| TUI 控制 | `/tui/{append-prompt,open-*,submit-prompt,clear-prompt,execute-command,show-toast,publish,select-session}` | 路径相同（v2 SDK 仍调用 v1 路径） |
| 实验特性 | `/experimental/{capabilities,console,console/orgs,console/switch,tool,tool/ids,worktree,session,session/:id/background}` | 同 v1 路径 |
| 认证 | `Authorization` 头 | 同 v1 路径（`Authorization` 头由 `ServerAuth.headers()` 注入） |

完整对照见第 3 节端点矩阵。

---

## 3. 完整端点矩阵

> 标识: ✅ 已实现 · 🔶 桩实现 · ✅ 已实现 · 🆕 v2 全新
> 引用: 文件:行号指向 opencode 主仓库的 spec 文档

### 3.1 Global 组（`protocols/http/global.md`）

| 方法 | 路径 | v2 路径 | 状态 | spec | 说明 |
| --- | --- | --- | --- | --- | --- |
| GET | `/global/health` | `/api/health` | ❌ | `global.md:27-49` | 返 `{healthy:true, version}` |
| GET | `/global/event` | `/api/event` | 🔶 | `global.md:52-101`、`sse-events.md:1-144` | 缺 `server.connected` / `server.heartbeat`、缺 `project/workspace` 字段 |
| GET | `/global/config` | 同 | ❌ | `global.md:103-119`、`config.md:124-164` | MVP: 路由到 `get_config` |
| PATCH | `/global/config` | 同 | ❌ | `global.md:121-147` | MVP: 写回 `~/.loom/config.toml` + 异步 `server.instance.disposed` |
| POST | `/global/dispose` | 同 | ❌ | `global.md:151-169` | 清空 `AppState` + 发 `server.instance.disposed` |
| POST | `/global/upgrade` | 同 | ❌ | `global.md:171-221` | 优先级低（MVP 跳过） |

### 3.2 Config 组（`protocols/http/config.md`）

| 方法 | 路径 | 状态 | spec | 说明 |
| --- | --- | --- | --- | --- |
| GET | `/config` | 🔶 | `config.md:24-51` | 当前返空 `{}`（`routes.rs:65-67`） |
| PATCH | `/config` | ❌ | `config.md:54-86` | 需新增 |
| GET | `/config/providers` | 🔶 | `config.md:89-120` | 返空 providers（`routes.rs:34-39`） |

### 3.3 Provider 组（`protocols/http/provider.md`）

| 方法 | 路径 | v2 路径 | 状态 | spec | 说明 |
| --- | --- | --- | --- | --- | --- |
| GET | `/provider` | `/api/provider` | 🔶 | `provider.md:17` | 返空数组（`routes.rs:43-45`） |
| GET | `/provider/auth` | 同 | ❌ | `provider.md:18`、`external-kernel-guide.md:138` | bootstrap 阻塞，返 `Record<id, AuthMethod[]>` |
| POST | `/provider/:id/oauth/authorize` | `/api/provider/{providerID}/oauth/authorize` | ❌ | `provider.md:19` | 返 `ProviderAuth.Authorization` |
| POST | `/provider/:id/oauth/callback` | `/api/provider/{providerID}/oauth/callback` | ❌ | `provider.md:20` | 返 `boolean` |

### 3.4 Agent / Model / Command / Skill / Reference / Integration 组

| 方法 | v1 路径 | v2 路径 | 状态 | spec | 说明 |
| --- | --- | --- | --- | --- | --- |
| GET | `/agent` | `/api/agent` | 🔶 | `tui-kernel-architecture.md:253-280` | 当前返 `[loom]`（`routes.rs:50-62`） |
| GET | — | `/api/model` | 🆕 | `tui-kernel-architecture.md:253-280` | v2 新增 |
| GET | `/command` | `/api/command` | ❌ | `external-kernel-guide.md:135` | bootstrap 非阻塞，返 `Command[]` |
| GET | — | `/api/skill` | 🆕 | `tui-kernel-architecture.md:253-280` | v2 新增 |
| GET | — | `/api/reference` | 🆕 | `tui-kernel-architecture.md:253-280` | v2 新增（取代 v1 `/experimental/resource`） |
| GET | — | `/api/integration` | 🆕 | `tui-kernel-architecture.md:253-280` | v2 新增 |

### 3.5 Path / Project / VCS 组

| 方法 | v1 路径 | v2 路径 | 状态 | spec | 说明 |
| --- | --- | --- | --- | --- | --- |
| GET | `/path` | 同 | 🔶 | `tui-kernel-architecture.md:253-280` | 当前返空（`routes.rs:74-76`） |
| GET | `/project` | `/api/location` | 🔶 | `workspace.md:13-23` | 当前返空（`routes.rs:81-83`） |
| GET | `/project/current` | `/api/location` | 🆕 | `workspace.md:13-23` | v1: 不存在，v2: 由 `/api/location` 承担 |
| GET | — | `/api/project/current` | 🆕 | v2 SDK 5398 行 | 返 `{ id, worktree?, vcs?, ... }` |
| GET | `/project/:id/directories` | `/api/project/{projectID}/directories` | ❌ | `workspace.md:13-23` | v2 命名变更 |
| GET | `/vcs` | `/api/vcs` | ❌ | `external-kernel-guide.md:143` | bootstrap 非阻塞，返 `VcsInfo` |
| — | — | `/api/vcs/status` | 🆕 | v2 SDK 2057 行 | v2 新增 |
| — | — | `/api/vcs/diff` | 🆕 | v2 SDK 2087 行 | v2 新增 |
| — | — | `/api/vcs/diff/raw` | 🆕 | v2 SDK 1995 行 | v2 新增 |
| — | — | `/api/vcs/apply` (POST) | 🆕 | v2 SDK 2121 行 | v2 新增 |

### 3.6 Session 组（`protocols/http/session.md`）

| 方法 | v1 路径 | v2 路径 | 状态 | spec |
| --- | --- | --- | --- | --- |
| GET | `/session` | `/api/session` | 🔶 | `session.md:17`、`tui-kernel-architecture.md:253-280` |
| POST | `/session` | `/api/session` | 🔶 | `session.md:23`、`session.md:62-73` Session.CreateInput |
| GET | `/session/status` | 同 | ❌ | `session.md:18`、`external-kernel-guide.md:136` |
| GET | `/session/:id` | `/api/session/{sessionID}` | 🔶 | `session.md:19` |
| PATCH | `/session/:id` | `/api/session/{sessionID}` | ❌ | `session.md:24` |
| DELETE | `/session/:id` | `/api/session/{sessionID}` | ❌ | `session.md:25` |
| GET | `/session/:id/children` | `/api/session/{sessionID}/children` | ❌ | `session.md:20` |
| GET | `/session/:id/todo` | `/api/session/{sessionID}/todo` | 🔶 | `session.md:21`、`routes.rs:129-131` |
| GET | `/session/:id/diff` | `/api/session/{sessionID}/diff` | 🔶 | `session.md:22`、`routes.rs:134-136` |
| GET | `/session/:id/messages` | `/api/session/{sessionID}/message` | 🔶 | `session.md:31`、`routes.rs:114-125` |
| GET | — | `/api/session/{sessionID}/message/{messageID}` | 🆕 | `session.md:32` |
| POST | `/session/:id/messages` | `/api/session/{sessionID}/message` | 🔶 | `session.md:33`、`routes.rs:190-295` |
| DELETE | — | `/api/session/{sessionID}/message/{messageID}` | ❌ | `session.md:37`（任务 8） |
| POST | `/session/:id/prompt` | `/api/session/{sessionID}/prompt` | 🔶 | `session.md:33`、`routes.rs:190-295` |
| POST | `/session/:id/prompt_async` | `/api/session/{sessionID}/prompt_async` | ❌ | `session.md:34` |
| POST | `/session/:id/command` | `/api/session/{sessionID}/command` | ❌ | `session.md:35` |
| POST | `/session/:id/shell` | `/api/session/{sessionID}/shell` | ❌ | `session.md:36` |
| POST | `/session/:id/abort` | `/api/session/{sessionID}/interrupt` | ❌ | `session.md:51`（任务 7） |
| POST | `/session/:id/fork` | `/api/session/{sessionID}/fork` | ❌ | `session.md:50`（任务 9） |
| POST | `/session/:id/init` | `/api/session/{sessionID}/init` | ❌ | `session.md:54`（任务 9） |
| POST | `/session/:id/summarize` | `/api/session/{sessionID}/summarize` | ❌ | `session.md:55`（任务 9） |
| POST | `/session/:id/share` | `/api/session/{sessionID}/share` | ❌ | `session.md:52`（任务 9） |
| DELETE | `/session/:id/share` | `/api/session/{sessionID}/share` | ❌ | `session.md:53`（任务 9） |
| POST | `/session/:id/revert` | `/api/session/{sessionID}/revert` | ❌ | `session.md:56`（任务 12） |
| POST | `/session/:id/unrevert` | `/api/session/{sessionID}/unrevert` | ❌ | `session.md:57`（任务 12） |
| POST | `/session/:id/permissions/:permissionID` | 同 | ❌ | `session.md:58` |
| PATCH | `/session/:id/message/:messageID/part/:partID` | `/api/session/{sessionID}/message/{messageID}/part/{partID}` | ❌ | `session.md:43` |
| DELETE | `/session/:id/message/:messageID/part/:partID` | 同 | ❌ | `session.md:44` |

### 3.7 Permission 组（`protocols/http/permission.md`）

| 方法 | v1 路径 | v2 路径 | 状态 | spec |
| --- | --- | --- | --- | --- |
| GET | `/permission` | `/api/permission` | ❌ | `permission.md:17` |
| POST | `/permission/:requestID/reply` | `/api/permission/{requestID}/reply` | ❌ | `permission.md:18`（任务 11） |
| GET | — | `/api/permission/saved` | 🆕 | `tui-kernel-architecture.md:253-280` |
| POST | — | `/api/permission/saved` | 🆕 | v2 新增 |
| DELETE | — | `/api/permission/saved/{id}` | 🆕 | v2 新增 |
| POST | — | `/api/permission/request` | 🆕 | v2 新增 |
| POST | — | `/api/session/{sessionID}/permission` | 🆕 | v2 新增 |
| POST | — | `/api/session/{sessionID}/permission/{requestID}` | 🆕 | v2 新增 |
| POST | — | `/api/session/{sessionID}/permission/{requestID}/reply` | 🆕 | v2 新增 |

### 3.8 Question 组（`protocols/http/question.md`）

| 方法 | v1 路径 | v2 路径 | 状态 | spec |
| --- | --- | --- | --- | --- |
| GET | `/question` | `/api/question` | ❌ | `question.md:17` |
| POST | `/question/:requestID/reply` | `/api/question/{requestID}/reply` | ❌ | `question.md:18`（任务 11） |
| POST | `/question/:requestID/reject` | `/api/question/{requestID}/reject` | ❌ | `question.md:19` |
| POST | — | `/api/question/request` | 🆕 | v2 新增 |
| POST | — | `/api/session/{sessionID}/question` | 🆕 | v2 新增 |
| POST | — | `/api/session/{sessionID}/question/{requestID}/reply` | 🆕 | v2 新增 |
| POST | — | `/api/session/{sessionID}/question/{requestID}/reject` | 🆕 | v2 新增 |

### 3.9 MCP / PTY / File / Find / Instance 组

| 方法 | v1 路径 | v2 路径 | 状态 | spec |
| --- | --- | --- | --- | --- |
| GET | `/mcp` | 同 | 🔶 | `mcp.md:17`、`routes.rs:308-310` |
| GET | `/mcp/status` | 同 | 🔶 | `mcp.md:17`、`external-kernel-guide.md:140`、`routes.rs:314-316` |
| POST | `/mcp`、`/mcp/:name/{auth,connect,disconnect,...}` | 同 | ❌ | `mcp.md:18-24` |
| GET | — | `/api/mcp` | 🆕 | v2 SDK 2419 行 |
| POST | — | `/api/mcp` | 🆕 | v2 SDK 2430 行 |
| POST | — | `/api/mcp/{name}/{connect,disconnect,auth,...}` | 🆕 | v2 SDK 2467-2518 行 |
| GET | `/pty/*` (8 端点) | `/api/pty/*` | ❌ | `pty.md:17-24` |
| GET | `/file`、`/file/content`、`/file/status` | `/api/file`、`/api/file/content`、`/api/file/status` | ❌ | v2 SDK 1829-1922 行 |
| GET | `/find`、`/find/file`、`/find/symbol` | `/api/find`、`/api/find/file`、`/api/find/symbol` | ❌ | v2 SDK 1725-1827 行 |
| GET | `/fs/read/*`、`/fs/list`、`/fs/find` | 同 | 🆕 | v2 SDK 6407-6499 行 |
| POST | `/instance/dispose` | `/api/instance/dispose` | ❌ | v2 SDK 1925-1956 行 |

### 3.10 TUI Control 组（`protocols/http/tui.md`）

| 方法 | 路径 | 状态 | spec | 说明 |
| --- | --- | --- | --- | --- |
| POST | `/tui/append-prompt` | ❌ | `tui.md:17` | 外部命令控制 TUI |
| POST | `/tui/open-help`、`/open-sessions`、`/open-themes`、`/open-models` | ❌ | `tui.md:18-21` | 同上 |
| POST | `/tui/submit-prompt`、`/clear-prompt` | ❌ | `tui.md:22-23` | 同上 |
| POST | `/tui/execute-command` | ❌ | `tui.md:24` | 同上 |
| POST | `/tui/show-toast` | ❌ | `tui.md:25` | 同上 |
| POST | `/tui/publish` | ❌ | `tui.md:26` | 同上 |
| POST | `/tui/select-session` | ❌ | `tui.md:27` | 同上 |
| GET | `/tui/control/next` | ❌ | `tui.md:28` | v2 新增（双向控制通道） |
| POST | `/tui/control/response` | ❌ | `tui.md:29` | 同上 |

### 3.11 Experimental 组（`protocols/http/experimental.md`）

| 方法 | 路径 | 状态 | spec | 说明 |
| --- | --- | --- | --- | --- |
| GET | `/experimental/capabilities` | ❌ | `experimental.md:17`、`external-kernel-guide.md:126` | bootstrap 阻塞，返 `{ backgroundSubagents: false }` |
| GET | `/experimental/console` | ❌ | `experimental.md:18`、`external-kernel-guide.md:127` | bootstrap 阻塞，返 `ConsoleState` |
| GET | `/experimental/console/orgs` | ❌ | `experimental.md:19` | 列出可切换组织 |
| POST | `/experimental/console/switch` | ❌ | `experimental.md:20` | 切换组织 |
| GET | `/experimental/tool` | ❌ | `experimental.md:21` | 列出工具（含参数 schema） |
| GET | `/experimental/tool/ids` | ❌ | `experimental.md:22` | 列出工具 ID |
| GET | `/experimental/worktree` | ❌ | `experimental.md:23` | 列出 Worktree |
| POST | `/experimental/worktree` | ❌ | `experimental.md:24` | 创建 Worktree |
| DELETE | `/experimental/worktree` | ❌ | `experimental.md:25` | 移除 Worktree |
| POST | `/experimental/worktree/reset` | ❌ | `experimental.md:26` | 重置 Worktree |
| GET | `/experimental/session` | ❌ | `experimental.md:27` | 跨项目列出会话 |
| POST | `/experimental/session/:id/background` | ❌ | `experimental.md:28` | 后台化子 Agent |
| GET | `/experimental/resource` | ❌ | `experimental.md:29`、`external-kernel-guide.md:141` | 返 MCP 资源；v2 改名为 `/api/reference` |

### 3.12 LSP / Formatter 组（v1 在 `external-kernel-guide.md:139-140` 提及，spec 未单独建文件）

| 方法 | v1 路径 | v2 路径 | 状态 | spec |
| --- | --- | --- | --- | --- |
| GET | `/lsp` | `/api/lsp` | ❌ | `external-kernel-guide.md:139` |
| GET | `/lsp/status` | `/api/lsp/status` | ❌ | 同上 |
| GET | `/formatter` | `/api/formatter` | ❌ | `external-kernel-guide.md:140` |
| GET | `/formatter/status` | `/api/formatter/status` | ❌ | 同上 |

---

## 4. 关键类型 Schema

> 全部为 v2 协议契约；v1 兼容版本主要差异在字段名（`snake_case` → `camelCase`）。

### 4.1 Session.Info

来源: `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\session.md:113-130`

```typescript
{
  id: SessionID,              // "sess_<uuid>"
  slug: string,
  projectID: ProjectV2.ID,
  workspaceID?: WorkspaceV2.ID,
  directory: string,
  path?: { cwd: string, root: string },
  parentID?: SessionID,
  title: string,
  agent?: string,
  model?: { id: ModelV2.ID, providerID: ProviderV2.ID, variant?: string },
  version: string,
  summary?: { additions, deletions, files, diffs? },
  cost?: number,
  tokens?: { input, output, reasoning, cache: { read, write } },
  share?: { url: string },
  metadata?: Record<string, any>,
  time: { created, updated, compacting?, archived? },
  permission?: PermissionV1.Ruleset,
  revert?: { messageID, partID?, snapshot?, diff? }
}
```

loom-server 当前 `state.rs:40-52` 缺 `slug/workspaceID/path/parentID/version/summary/cost/tokens/share/metadata/permission/revert`。P0.5 任务负责补齐。

### 4.2 PromptPayload

来源: `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\session.md:75-93`

```typescript
{
  messageID?: MessageID,
  model?: { id, providerID, variant? },
  agent?: string,
  noReply?: boolean,
  tools?: Record<string, boolean>,  // deprecated
  format?: SessionV1.Format,
  system?: string,
  variant?: string,
  parts: Array<
    | { type: "text", text: string, ... }
    | { type: "file", mime: string, url: string, ... }
    | { type: "agent", name: string, ... }
    | { type: "subtask", prompt: string, description: string, agent: string, ... }
  >
}
```

loom-server `routes.rs:198-207` 当前仅识别 `{ type: "text" }`，文件/agent/subtask 落入未来阶段。P1.6 任务分流 v1/v2 路径。

### 4.3 Part 类型枚举

来源: `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\README.md` 关联 + `C:\Users\heycj\dev\opencode\packages\sdk\js\src\v2\gen\types.gen.ts:2850-2920`

```
text | reasoning | tool | file | step-start | step-finish | snapshot |
patch | agent | compaction | retry | subtask
```

loom-server `translator.rs:33-130` 当前仅处理 `text` / `reasoning` / `tool`；其余 9 种 P0.4 后扩展。

### 4.4 SessionStatus

来源: `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\session.md:105-111`

```typescript
| { type: "idle" }
| { type: "busy" }
| { type: "retry", attempt: number, message: string,
    action?: { reason, provider, title, message, label, link? }, next: number }
```

loom-server `routes.rs:259-262` + `agent_runner.rs:150-153` 当前仅发 `busy` / `idle`；`retry` 留给未来 LLM 失败重试。

### 4.5 PermissionV1.Request

来源: `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\permission.md:20-29`

```typescript
{
  id: PermissionV1.ID, sessionID: SessionID,
  permission: string, patterns: string[],
  metadata: Record<string, unknown>,
  always: string[],
  tool?: { messageID: string, callID: string }
}
```

### 4.6 Question.Request

来源: `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\question.md:21-32`

```typescript
{
  id: QuestionID, sessionID: SessionID,
  questions: Array<{
    question: string, header: string,        // header max 30 chars
    options: Array<{ label: string, description: string }>,
    multiple?: boolean, custom?: boolean      // custom default: true
  }>,
  tool?: { messageID: SessionV1.MessageID, callID: string }
}
```

### 4.7 GlobalEventSchema（v2 SSE envelope）

来源: `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\global.md:263-279`

```typescript
type GlobalEvent = {
  directory: string,        // 必填
  project?: string,         // 可选
  workspace?: string,       // 可选（workspaceID）
  payload: union            // 见下
}

payload union:
  | EventManifest    { id: EventV2.ID, type: string, properties: Schema }
  | InstanceDisposed { id, type: 'server.instance.disposed', properties: { directory } }
  | SyncEvent        { type: 'sync', id, syncEvent: { type, id, seq, aggregateID, data } }
```

loom-server `state.rs:99-111` 当前 envelope 缺 `project/workspace` 字段；P0.3 任务负责扩展。

---

## 5. 现有 v1 handler 迁移表

> 状态: ✅ 不动 · 🔧 改 envelope/字段 · ➕ 新增 handler

| 现有路径 | `routes.rs` 行 | 状态 | 改动 |
| --- | --- | --- | --- |
| `GET /config/providers` | `routes.rs:34-39` | 🔧 | 保持路径；从 `~/.loom/config.toml` 读 `[[providers]]`，映射为 `Provider.Info[]` |
| `GET /provider` | `routes.rs:43-45` | 🔧 | v1 路径保留；新增 v2 alias `/api/provider` 共享 handler |
| `GET /agent` | `routes.rs:50-62` | 🔧 | v1 路径保留；新增 `/api/agent`；返 `AgentV2Info[]`（含 `mode/permissions`） |
| `GET /config` | `routes.rs:65-67` | 🔧 | 改返 `ConfigV1.Info`（当前 `{}`），字段见 4.1 |
| `GET /path` | `routes.rs:74-76` | 🔧 | 改返 `{ home, state, config, worktree, directory }`（v1 fixture `tui-sdk.ts:89` 要求） |
| `GET /project` | `routes.rs:81-83` | 🔧 | 路径保留兼容 v1；新增 v2 `/api/location`（优先） |
| `GET /session` | `routes.rs:89-97` | ✅ | 透传 v1 + v2 alias |
| `POST /session` | `routes.rs:139-177` | 🔧 | 改返 `Session.Info`（4.1） |
| `GET /session/:id` | `routes.rs:100-109` | 🔧 | 同上 |
| `GET /session/:id/messages` | `routes.rs:114-125` | 🔧 | 改返 `SessionV1.WithParts[]`（v2: `SessionMessage[]`） |
| `GET /session/:id/todo` | `routes.rs:129-131` | ✅ | 留空数组 |
| `GET /session/:id/diff` | `routes.rs:134-136` | ✅ | 留空数组 |
| `POST /session/:id/prompt` | `routes.rs:190-295` | 🔧 | P1.6 共享 handler；v2 alias `/api/session/:id/prompt` |
| `GET /mcp` | `routes.rs:308-310` | ✅ | 保留 `{ data: {} }` |
| `GET /mcp/status` | `routes.rs:314-316` | ✅ | 保留（legacy） |
| `GET /global/event` | `sse.rs:24-40` | 🔧 | P0.3 拆出 v1/v2 双通道；补 `server.connected` / `server.heartbeat` |

总计 15 条现有 handler，10 条需要改动，5 条直接保留。

---

## 6. 任务清单（25 项）

按依赖顺序排列；每条引用对应 spec 章节与目标代码位置。

### P0 — bootstrap 阻塞（高）

1. [x] **依赖** 给 loom-server 追加 `clap`、`tower-http`，保留 `Cargo.toml:23-40` 中 `apps/server` 注册。参考 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\1-architecture\tui-kernel-architecture.md:9-90`（双进程模型）。
2. [x] **v2 路由占位** 在 `apps/server/src/main.rs:34-58` 与 `apps/server/src/routes.rs:1-316` 注册：`/api/location`、`/api/agent`、`/api/model`、`/api/provider`、`/api/command`、`/api/skill`、`/api/reference`、`/api/integration`、`/vcs`、`/vcs/status`、`/vcs/diff`、`/vcs/diff/raw`、`/api/fs/list`、`/api/fs/find`、`/api/health`、`/api/permission/saved`。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\3-implementation\external-kernel-guide.md:112-158` + `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\1-architecture\tui-kernel-architecture.md:253-280`。
3. [x] **v1 补齐 bootstrap** 注册 `/session/status`、`/provider/auth`、`/lsp/status`、`/formatter/status`、`/experimental/capabilities`、`/experimental/console`、`/experimental/console/orgs`、`/experimental/resource`。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\3-implementation\external-kernel-guide.md:116-143`。
4. [x] **双格式 SSE envelope** 扩展 `apps/server/src/state.rs:99-148` 中的 `GlobalEvent` 增加 `project?/workspace?`；`apps/server/src/sse.rs:1-41` 拆出 `/api/event`（v2）与 `/global/event`（v1）两条 SSE 通道，共享广播总线但各自序列化；新增 `server.connected` 初始事件 + `server.heartbeat` 10s 心跳。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\global.md:52-101` + `:263-279` + `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\sse-events.md:25-111`。
5. [x] **translator 升级** `apps/server/src/translator.rs:1-130` 输出的事件名对齐 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\eventv2.md:11-184` 事件目录（Session/Step/Text/Reasoning/Tool/Shell/Compaction/Revert/Permission/Question/Todo/Status/Infra/Workspace/TUI），保留 v1 字段别名。
6. [x] **SessionInfo/MessageInfo 形状** 补齐 `apps/server/src/routes.rs:99-177` 与 `apps/server/src/state.rs:39-77`，输出 `parentID/slug/workspaceID/path/cost/tokens/share/permission/revert`。依据 4.1 + `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\session.md:113-130` + `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\2-contracts\tui-agent-interaction.md:49-130`。
7. [x] **认证头兼容** 在 `apps/server/src/main.rs:34-58` 之前加 `tower-http` 的中间件，校验或忽略 `Authorization` 头（`ServerAuth.headers()`，见 `C:\Users\heycj\dev\opencode\packages\opencode\src\cli\cmd\tui.ts:236`）。MVP 默认放行任何值。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\3-implementation\external-kernel-guide.md:254-264`。

### P1 — 交互主路径（高）

8. [x] **POST /api/session/:id/prompt** 与 v1 `/session/:id/prompt` 共享 `apps/server/src/routes.rs:190-295` handler，按 `Content-Type` 或 `?v=2` 分流；解析 `PromptPayload`（4.2），错误体走 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\README.md:109-122`。端到端参考 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\3-implementation\tui-interaction-trace.md:9-123`（动作 1）。
9. [x] **POST /session/:id/command + /api/session/{sessionID}/command** 新增 handler；解析 `CommandPayload`（`C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\session.md:35`），MVP 直接走 ReAct prompt。端到端参考 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\3-implementation\tui-interaction-trace.md:511-552`（动作 7）。
10. [x] **POST /session/:id/shell + /api/session/{sessionID}/shell** 新增 handler；MVP 走 `LocalCommandExecutor` 直跑（不调 LLM）；`ShellPayload` 见 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\session.md:95-103`。端到端参考 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\3-implementation\tui-interaction-trace.md:553-554`（动作 8）。
11. [x] **中止通道** 注册 `POST /session/:id/abort`（v1）+ `POST /api/session/{id}/interrupt`（v2）。把 `apps/server/src/agent_runner.rs:40-82` 的占位 `RunCancellation` 替换为 `AppState` 中 per-session token（`Arc<RwLock<HashMap<SessionID, CancellationToken>>>`），handler 触发后 `cancel()` 并广播 `session.status{idle}`。参考 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\3-implementation\tui-interaction-trace.md:483-509`（动作 6）+ `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\2-contracts\tui-agent-interaction.md:465-519`。
12. [x] **删除消息** `DELETE /api/session/{id}/message/{messageID}`（v2）+ v1 兼容路径；改写 `state.messages/parts` 后广播 `message.removed` + `message.part.removed`。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\session.md:32-37` + `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\eventv2.md:78-95`。
13. [x] **share / fork / summarize / init / revert(un)** 新增 handlers（`Session.Info` / `boolean` 返回）。fork 复制 `messages+parts`；share 仅返回 `share.url` 占位；init/summarize 返回成功。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\session.md:46-58` + `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\2-contracts\tui-agent-interaction.md:285-325`。
14. [x] **session CRUD 补齐** PATCH `/session/:id`（更新 title/agent）、DELETE `/session/:id`、GET `/session/:id/children`。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\session.md:24-25,20` + `C:\Users\heycj\dev\opencode\packages\sdk\js\src\v2\gen\sdk.gen.ts:3559-3608`。
15. [x] **Part CRUD** PATCH/DELETE `/api/session/{id}/message/{msgID}/part/{partID}`。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\session.md:43-44`。
16. [x] **TUI control 端点** 注册 `/tui/{append-prompt, open-help, open-sessions, open-themes, open-models, submit-prompt, clear-prompt, execute-command, show-toast, publish, select-session}` + `/tui/control/{next, response}`。MVP 占位返 204。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\tui.md:17-29`。

### P2 — 次要交互（中）

17. [x] **v2 增量回放** `GET /api/session/:id/event`。在 `AppState` 引入 `Arc<RwLock<VecDeque<GlobalEvent>>>` 缓存最近 N 条，按 `id` 去重（`types.gen.ts:7247` `EventManifest.id`）。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\sse-events.md:75-91` + `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\eventv2.md:230-249`。
18. [x] **permission / question** 注册 `/api/permission/{request, saved, saved/{id}, {requestID}/reply}`、`/api/question/{request, {requestID}/reply, {requestID}/reject}`、v1 兼容 `/permission/{requestID}/reply`、`/question/{requestID}/reply|reject`、`/permission`、`/question`。MVP 返回空 envelope。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\permission.md:13-30` + `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\question.md:13-33` + `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\2-contracts\tui-agent-interaction.md:371-463`。
19. [x] **revert stage/clear/commit** v2 路径 `/api/session/{id}/revert/{stage,clear,commit}` + v1 兼容。MVP 返回成功 envelope，不改写文件。参考 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\eventv2.md:89-95` + `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\session.md:56-57`。
20. [x] **MCP / PTY / File / Find** 注册 v2 alias + v1 兼容（`/mcp`、`/mcp/:name/{auth,connect,disconnect}`、`/pty/*` 8 端点、`/file*`、`/find*`、`/instance/dispose`）。MVP 全部返 200/204 + 空 envelope。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\mcp.md:17-24` + `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\pty.md:17-24` + v2 SDK `find`/`file` 段。
21. [x] **Experimental 完整集** 注册 `/experimental/{tool, tool/ids, worktree*, session, session/:id/background, console/switch, capabilities}` 全部路径。MVP 返空 envelope。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\experimental.md:17-29`。
22. [x] **Provider OAuth** 注册 `/provider/:id/oauth/{authorize, callback}`。MVP 返 501（未实现），后续接 loom 自身 OAuth 流。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\provider.md:19-20`。
23. [x] **Global control** 注册 `/global/{health, config (GET/PATCH), dispose}`。`/global/health` 返 `{healthy:true, version}`；`PATCH /global/config` 写回 `~/.loom/config.toml` + 异步 `server.instance.disposed`；`/global/dispose` 清空 `AppState` + 发 `server.instance.disposed`。`/global/upgrade` 标 MVP-skip。依据 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\http\global.md:27-169`。

### P3 — 验证（中/低）

24. [x] **curl 集成脚本** `scripts/check-protocol.sh` 遍历 `external-kernel-guide.md:112-158` 表中所有路径，断言 2xx + envelope 形状。参考 `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\3-implementation\external-kernel-guide.md:265-296`（数据契约、事件契约、状态契约、SSE 契约 4 类检查）。
25. [x] **TUI 冒烟 + 单元测试 + 文档同步 + 提交清理**（合并 P3.13-17）：
    - 启动 `bunx tui --server http://localhost:3000`，或扩展 `C:\Users\heycj\dev\opencode\packages\tui\test\fixture\tui-sdk.ts:68-106`（已支持 v2 路径 fixture）
    - `apps/server/src/{routes,sse,translator}/tests.rs` 覆盖 v1+v2 envelope、abort、share、auth
    - 更新 `docs/opencode-protocol/acp-adjacent/rust-agent-client-protocol-index.md`，标记 loom-server 已实现的 v1/v2 端点，引用本文档
    - 依 `git-commit-workflow` 拆为 5 个 commit；提交前 `cargo clippy -p loom-server --all-targets -- -D warnings` 与 `cargo fmt` 通过

---

## 7. 依赖关系

```
P0.1 依赖 ──┬─> P0.2 ──> P1.8
            ├─> P0.3 ──> P1.8 / P2.20
            ├─> P0.4 ──> P1.8 / P1.11
            ├─> P0.5 ──> P1.8 / P1.12
            └─> P0.7 ──> P1.* (所有需要 auth 的)

P0.* 全部完成 ──> P1.8-P1.16
P1.11 完成    ──> P1.12
P1.* 全部完成 ──> P2.17-P2.23
P1.11 完成    ──> P3.24 (curl 校验 abort 行为)

P0.6 (SessionInfo) ──> P1.13 (share/fork 返 Session.Info)
P0.6              ──> P1.14 (CRUD 返 Session.Info)

P3.24 通过 ──> P3.25 (冒烟 + 单元 + 同步 + 提交)
```

强约束:

- P0.4（双 envelope）必须先于 P0.5（translator）和 P1.11（abort）：后两者依赖 envelope 形状确定
- P0.2 + P0.3（v2 路由 + v1 补齐）必须先于 P1.8（prompt）：避免 prompt handler 转发时目标路径返回 404
- P0.6（SessionInfo 形状）必须先于 P1.13 / P1.14：CRUD/share/fork 返完整 Session.Info
- P0.7（auth）必须先于 P1.*：所有需要 auth 的交互路径都依赖
- P1.11（abort）必须先于 P1.12（删除消息）：删除消息不能在 in-flight run 中产生 race
- P3.24 / P3.25 任一失败则不进入提交

---

## 8. 实施节奏（按 PR 切分）

> 每个 PR 必须通过 `cargo build -p loom-server`、`cargo test -p loom-server`、`cargo clippy -p loom-server --all-targets -- -D warnings`、`cargo fmt --check`、`scripts/check-protocol.sh`。

### PR-1: bootstrap 基础（≈1.5 d）

- 范围: P0.1, P0.2, P0.3, P0.5, P0.6
- 验证: `bunx tui --server http://localhost:3000` 启动后能进入主界面（`data.tsx:551-560` 8 个 `refresh` 不再 throw）
- 风险: 低，纯占位 + envelope 扩展

### PR-2: SSE 双通道 + 认证（≈1 d）

- 范围: P0.4, P0.7
- 验证: 同时订阅 v1/v2 两条 SSE，验证 envelope 字段、heartbeat 节奏、TUI 重连不卡
- 风险: 中，`serde` union 引入复杂度；10s 节奏需要在 `sse.rs` 单独计时器

### PR-3: 交互主路径（≈2 d）

- 范围: P1.8, P1.9, P1.10, P1.11, P1.12
- 验证: TUI 端到端：输入 prompt → 流式输出 → 工具调用 → 状态条变化 → abort 中止 → 删消息
- 风险: 中高，abort 涉及 `RunCancellation` 全局状态改造

### PR-4: session 全生命周期（≈1.5 d）

- 范围: P1.13, P1.14, P1.15, P1.16
- 验证: session 列表/创建/PATCH/删除/fork/share/TUI 控制全部返 2xx
- 风险: 低

### PR-5: 次要交互 + 验证 + 文档（≈2 d）

- 范围: P2.17-P2.23 + P3.24, P3.25
- 验证: `scripts/check-protocol.sh` 全绿；`bunx tui` 走完 10 个交互动作（`tui-interaction-trace.md:11-625`）；`docs/opencode-protocol/acp-adjacent/rust-agent-client-protocol-index.md` 同步更新
- 风险: 低

总计 ≈ 8 d 单线程。

---

## 9. 关键文件锚点

- `Cargo.toml:23-40` 工作区成员注册
- `apps/server/Cargo.toml:1-35` loom-server 依赖
- `apps/server/src/main.rs:34-58` 路由注册表
- `apps/server/src/state.rs:99-148` `GlobalEvent` envelope
- `apps/server/src/state.rs:39-77` SessionInfo / MessageInfo / PartInfo
- `apps/server/src/sse.rs:1-41` SSE 序列化（需扩展 heartbeat/双通道）
- `apps/server/src/translator.rs:1-130` StreamEvent → V2Event
- `apps/server/src/routes.rs:1-316` HTTP handlers
- `apps/server/src/agent_runner.rs:1-156` ReAct 入口（P1.11 改 cancellation）

---

## 10. 认证处理

来源: `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\3-implementation\external-kernel-guide.md:254-264`

External 模式下 TUI 通过 `ServerAuth.headers()` 注入 `Authorization` 头（`C:\Users\heycj\dev\opencode\packages\opencode\src\cli\cmd\tui.ts:236`）。loom-server 必须处理或忽略该头。

实现策略:

- **MVP（PR-2）**: 在 `apps/server/src/main.rs` 加 `tower-http` 的 `SetResponseHeaderLayer` / 自定义 middleware，记录头但放行。日志级别 `debug`。
- **生产（后续 PR）**: 若启用 opencode 的 `OPENCODE_SERVER_PASSWORD` 或 `OPENCODE_SERVER_USERNAME` env，loom-server 应校验 `Authorization: Basic <b64>` 或 `Bearer <token>`。校验失败返 401。
- **本地开发**: `Authorization` 头缺失时，行为同放行。

loom-server 默认不强制认证，PR-2 落地中间件以避免后续忘记添加。

---

## 11. SSE 心跳与 16ms 批处理

### 11.1 心跳节奏

来源: `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\sse-events.md:32-56`

```
T+0s    : server.connected 推送（连接建立立即）
T+10s   : server.heartbeat 推送
T+20s   : server.heartbeat 推送
...
T+30s   : server.heartbeat 推送
T+Tx    : 业务事件
```

loom-server `sse.rs:36-40` 当前用 `KeepAlive::new().interval(10s).text("keepalive")` 走 SSE 注释行（`: keepalive`），这与 spec 要求的 `server.heartbeat` 业务事件不一致：

- 注释行 = TCP keepalive（防代理切断）
- `server.heartbeat` 业务事件 = TUI 端能识别并触发重连逻辑

P0.4 任务负责拆为两条逻辑：

- TCP 层：保留 axum `KeepAlive` 注释行
- 业务层：在 `event_tx` 注入 10s 间隔的 `server.heartbeat` 事件（payload `{ id, type: "server.heartbeat", properties: {} }`）

### 11.2 16ms 批处理

来源: `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\protocols\sse-events.md:112-135` + `C:\Users\heycj\dev\opencode\specs\v2\loom-kernel\1-architecture\tui-kernel-architecture.md:147-200`

TUI SDK 客户端（`C:\Users\heycj\dev\opencode\packages\tui\src\context\sdk.tsx:48-68`）在收到事件后做 16ms 窗口合并：

- 单个事件立即 flush
- 16ms 内累积的事件批量 emit，触发 SolidJS 单次重渲染

loom-server 端的影响：

- 不需要在服务端做 16ms 合并（**服务端职责是"尽快发送"**）
- 但要避免发送过快导致 TUI 端 backpressure：`emit()` 后让 tokio runtime 调度
- 业务事件流（`message.part.delta`）可能非常密集（每 token 一次），服务端应保持异步不阻塞

P0.4 不需要新增服务端合并逻辑，只需保证 emit 路径无锁竞争（`tokio::sync::broadcast` 已是非阻塞的）。

---

## 12. 风险与备选

| 风险 | 影响 | 备选 |
| --- | --- | --- |
| 双 envelope 引入 `serde` union，工作量 ≈ 0.5 d | 短期 | 仅发 v2 envelope，在 opencode 端做 v1 兼容；opencode 仍发布 v1 TUI 时风险大 |
| `throwOnError: true` 触发硬失败时 TUI 渲染层访问 `undefined.entries()` 崩溃 | v2 bootstrap 全挂 | 占位返回 `{ data: { items: [] } }` 而非裸 `[]`，等手测后调 |
| 取消通道涉及 `RunCancellation` 全局状态 | P1.11 阻塞 | 落地前 `grep -R "RunCancellation" agent/` 确认所有使用点 |
| v1 TUI 仍可能在某占位返 `[]` 后渲染期崩溃 | v1 退场 | TUI 主分支已切 v2，仅维护期可能回退 v1 兼容 |
| 服务端 10s heartbeat 与 axum 注释 keepalive 重复 | 网络流量 ×2 | 业务层不推 heartbeat，仅用注释行（违反 spec，**不推荐**） |
| PR-1 完成后端点矩阵 95% 已就位但 handler 全是空 envelope | 误以为完工 | `scripts/check-protocol.sh` 校验 envelope 字段非 `null/undefined/[]` |
| `apps/server` 还未 commit，独立 PR 与已存在 `tool-luft` 提交冲突 | 提交时冲突 | `git rebase dev` 前先 rebase 目标；PR-1 单独 cherry-pick |
| `Cargo.toml` 合并符号不一致（`"apps/cli",` vs `"apps/cli"`） | rustfmt 不通过 | PR-1 前先 `cargo fmt --check` 修齐；参考 `Cargo.toml:23-40` |

---

## 13. 验证策略

1. **单元** `cargo test -p loom-server`（envelope 双格式、abort、share、auth 头忽略）
2. **集成** `scripts/check-protocol.sh`（4 类契约：数据 / 事件 / 状态 / SSE）
3. **冒烟** `bunx tui --server http://localhost:3000`，走完 10 个交互动作（`tui-interaction-trace.md`）
4. **回归** `cargo clippy -p loom-server --all-targets -- -D warnings`
5. **Lint** `cargo fmt --check`
6. **Worktree 清理** `git status` 不应有 `_inspect_*.rs` 调试残留

### 13.1 数据契约断言（`external-kernel-guide.md:269-272`）

- `GET /session` 返 `SessionInfo[]`，字段含 `id/title/agent/path/time`
- `GET /agent` 返 `Agent[]`，字段含 `name/description`
- `POST /session` 返 `{ id: string }`（`sess_` 前缀）
- `POST /session/:id/prompt` 返 200 后异步推送事件

### 13.2 事件契约断言（`external-kernel-guide.md:276-281`）

- `message.updated` — 先创建 message 行，再推送 part
- `message.part.delta` — `delta` 是增量文本，TUI 做字符串拼接
- `message.part.updated` — 整个 Part 对象替换，不是增量
- `session.status` — busy / idle 必须配对
- `permission.asked` / `permission.replied` — 配对出现
- `finish` 字段在最后一次 `message.updated` 中带上

### 13.3 状态契约断言（`external-kernel-guide.md:284-288`）

- prompt HTTP 调用立即返回，不等 LLM 结果
- 所有 LLM 输出通过 SSE 事件流推送
- Part 的 `type` 字段只使用已定义的枚举值
- ToolPart `state.status` 遵守 `pending → running → completed | error` 状态机

### 13.4 SSE 契约断言（`external-kernel-guide.md:292-295`）

- `Content-Type: text/event-stream`
- 每个 `data:` 行尾 `\n\n`
- 10s 心跳
- 连接断开后 TUI 会指数退避重连

---

## 14. 引用

### 14.1 设计文档（loom 主仓库 `specs/v2/loom-kernel/`）

- `README.md:1-59`
- `1-architecture/tui-kernel-architecture.md:1-338`
- `2-contracts/protocol-specification.md:1-65`
- `2-contracts/tui-agent-interaction.md:1-703`

### 14.2 HTTP 协议

- `protocols/README.md:1-201`
- `protocols/http/global.md:1-297`
- `protocols/http/config.md:1-208`
- `protocols/http/session.md:1-130`
- `protocols/http/workspace.md:1-23`
- `protocols/http/permission.md:1-30`
- `protocols/http/question.md:1-33`
- `protocols/http/experimental.md:1-29`
- `protocols/http/mcp.md:1-24`
- `protocols/http/pty.md:1-24`
- `protocols/http/provider.md:1-20`
- `protocols/http/tui.md:1-29`

### 14.3 事件

- `protocols/sse-events.md:1-144`
- `protocols/eventv2.md:1-278`
- `protocols/globalbus.md:1-113`
- `protocols/worker-rpc.md:1-129`

### 14.4 实现指引

- `3-implementation/external-kernel-guide.md:1-421`
- `3-implementation/tui-interaction-trace.md:1-625`
- `3-implementation/loom-kernel-shortest-path.md:1-79`

### 14.5 TUI 代码锚点（opencode 主仓库 `packages/tui/`）

- `src/context/sdk.tsx:1-130` v2 SDK 客户端封装
- `src/context/data.tsx:405-560` bootstrap 数据加载
- `src/context/data.tsx:124-411` V2Event handler
- `src/context/project.tsx:38-50` project sync
- `src/context/event.ts:1-50` 事件订阅
- `test/fixture/tui-sdk.ts:68-106` v2 路径 fixture

### 14.6 v2 SDK 类型定义

- `C:\Users\heycj\dev\opencode\packages\sdk\js\src\v2\gen\sdk.gen.ts:454-7219` 类与方法
- `C:\Users\heycj\dev\opencode\packages\sdk\js\src\v2\gen\types.gen.ts:730-820` GlobalEvent
- `C:\Users\heycj\dev\opencode\packages\sdk\js\src\v2\gen\types.gen.ts:2850-2920` V2Event 联合类型
- `C:\Users\heycj\dev\opencode\packages\sdk\js\src\v2\gen\types.gen.ts:9804-9813` PromptPayload

### 14.7 loom-server 内部实现状态（spec 自审）

- `protocols/http/global.md:284-296` Global 组状态
- `protocols/http/config.md:198-208` Config 组状态
- `protocols/http/global.md:291` `server.connected` / `server.heartbeat` 缺失说明
- `protocols/http/global.md:291` envelope 缺 `project/workspace` 说明
