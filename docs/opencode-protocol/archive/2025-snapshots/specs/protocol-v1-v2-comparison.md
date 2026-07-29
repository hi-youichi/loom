# opencode 协议端点完整矩阵

> 以 opencode 源码为权威来源，列出全部端点及其在 loom-server 中的实现状态。
>
> 数据源：
> - v1 实例路由: `packages/opencode/src/server/routes/instance/httpapi/groups/*.ts`
> - v2 协议路由: `packages/protocol/src/groups/*.ts`
> - SDK 客户端: `packages/sdk/js/src/v2/gen/sdk.gen.ts`
>
> opencode 服务器同时挂载两套 API 面（`server.ts:276-281`）：
> - **RootHttpApi** + **InstanceHttpApi** = v1 实例路由（无 `/api` 前缀）
> - **ServerApi** (protocol) = v2 协议路由（`/api` 前缀）

## 架构

```
                     opencode 原始服务器
                     ┌─────────────────────────────────────────────┐
                     │  v1 实例路由 (Instance API)                  │
                     │    /agent /session/* /provider /config ...   │
                     │                                             │
                     │  v2 协议路由 (Server API)                    │
                     │    /api/session/* /api/provider /api/model   │
                     └─────────────────────────────────────────────┘
                           ▲                          ▲
                           │ 直连                      │
   ┌───────────────┐       │                          │
   │  opencode TUI │───────┘                          │
   │  (SDK v2)     │                                  │
   └───────────────┘                                  │
                      ┌──────────────────┐            │
                      │ OpenChamber      │            │
                      │ Express Proxy    │            │
                      │ pathRewrite:     │            │
                      │   ^/api → ""     │            │
                      └────────┬─────────┘            │
                               │                      │
                      ┌────────▼─────────┐            │
                      │  OpenChamber     │────────────┘
                      │  Browser (SDK v2)│
                      └──────────────────┘
```

## 代理路径规则（OpenChamber）

OpenChamber 的 Express 代理执行 `pathRewrite: ^/api → ""`，只剥离**第一个** `/api`。
SDK 客户端有两种 URL：

| SDK URL 格式 | 浏览器发送 | 代理剥离后 | loom-server 收到 |
|---|---|---|---|
| `/agent` (v1) | `/api/agent` | `/agent` | `/agent` |
| `/session/:id` (v1) | `/api/session/:id` | `/session/:id` | `/session/:id` |
| `/api/session/:id` (v2) | `/api/api/session/:id` | `/api/session/:id` | `/api/session/:id` |
| `/api/model` (v2) | `/api/api/model` | `/api/model` | `/api/model` |

loom-server 需为 v1 路径和 v2 路径各注册一份路由。

## SSE 通道

| 通道 | v1 | v2 | 信封差异 |
|------|----|----|---------|
| 全局事件 | `GET /global/event` | `GET /api/event` | v1: `{directory, payload:{type, properties}}`；v2: 额外含 `project?`, `workspace?`, `payload.id` |
| 实例事件 | `GET /event` | — | 同 v1 格式 |
| 会话事件 | — | `GET /api/session/:id/event?after=cursor` | v2 独有，支持增量回放 |

---

## 端点完整清单

图例：
- **v1** = 实例路由，无 `/api` 前缀
- **v2** = 协议路由，`/api` 前缀
- **loom** = loom-server 实现状态：✅ 已实现 ｜ ⚠️ 双注册 handler 不同 ｜ stub 占位 ｜ ❌ 未实现 ｜ 🔁 v1/v2 共用

### Global / Root

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/global/health` | `global.health` | ✅ | OpenChamber 启动握手探针 |
| GET | `/global/event` | `global.event` | ✅ | v1 SSE 广播 |
| GET | `/global/config` | `config.get` | ✅ | |
| PATCH | `/global/config` | `config.update` | ✅ | |
| POST | `/global/dispose` | `global.dispose` | ✅ | |
| POST | `/global/upgrade` | `global.upgrade` | 501 | stub |

### Control

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| PUT | `/auth/:providerID` | `auth.set` | ❌ | 未实现 |
| DELETE | `/auth/:providerID` | `auth.remove` | ❌ | 未实现 |
| POST | `/log` | `app.log` | ❌ | 未实现 |

### Control Plane

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| POST | `/experimental/control-plane/move-session` | `controlPlane.moveSession` | ❌ | 未实现 |

### App (v1 实例元数据)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/agent` | `app.agents` | ✅ | `get_agent_list`，返回 `{id, name, ...}` |
| GET | `/skill` | `app.skills` | ✅ | `get_api_skills` |
| POST | `/log` | `app.log` | ❌ | 未实现 |

### Config (v1)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/config` | `config2.get` | ✅ | `get_api_config` |
| PATCH | `/config` | `config2.update` | ✅ | `patch_api_config` |
| GET | `/config/providers` | `config2.providers` | ✅ | `get_config_providers` |

### Event (v1)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/event` | `event.subscribe` | ✅ | 实例级 SSE |

### Instance (v1)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| POST | `/instance/dispose` | `instance.dispose` | ✅ | |
| GET | `/path` | `path.get` | ✅ | `get_api_path` |
| GET | `/vcs` | `vcs.get` | ✅ | |
| GET | `/vcs/status` | `vcs.status` | ✅ | |
| GET | `/vcs/diff` | `vcs.diff` | ✅ | |
| GET | `/vcs/diff/raw` | `diff.raw` | ✅ | |
| POST | `/vcs/apply` | `vcs.apply` | ❌ | 未实现 |
| GET | `/command` | `command.list` | ✅ | ⚠️ v1/v2 handler 不同 |
| GET | `/lsp` | `lsp.status` | ✅ | |
| GET | `/formatter` | `formatter.status` | ✅ | |

### File (v1)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/file` | `file.list` | ✅ | ⚠️ v1/v2 handler 不同 |
| GET | `/file/content` | `file.read` | ✅ | |
| GET | `/file/status` | `file.status` | ✅ | |
| GET | `/find` | `find.text` | ✅ | ⚠️ v1/v2 handler 不同 |
| GET | `/find/file` | `find.files` | ✅ | ⚠️ v1/v2 handler 不同 |
| GET | `/find/symbol` | `find.symbols` | ✅ | ⚠️ v1/v2 handler 不同 |

### MCP (v1)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/mcp` | `mcp.status` | ✅ | |
| POST | `/mcp` | `mcp.add` | stub | `true_value` |
| POST | `/mcp/:name/auth` | `auth2.start` | ⚠️ | v1/v2 handler 不同 |
| DELETE | `/mcp/:name/auth` | `auth2.remove` | stub | `true_value` |
| POST | `/mcp/:name/auth/callback` | `auth2.callback` | stub | `true_value` |
| POST | `/mcp/:name/auth/authenticate` | `auth2.authenticate` | ❌ | 未实现 |
| POST | `/mcp/:name/connect` | `mcp.connect` | ❌ | 未实现 |
| POST | `/mcp/:name/disconnect` | `mcp.disconnect` | ❌ | 未实现 |

### Project (v1)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/project` | `project.list` | ⚠️ | v1/v2 handler 不同 |
| GET | `/project/current` | `project.current` | ⚠️ | v1/v2 handler 不同 |
| POST | `/project/git/init` | `project.initGit` | ❌ | 未实现 |
| PATCH | `/project/:projectID` | `project.update` | ❌ | 未实现 |
| GET | `/project/:projectID/directories` | `project.directories` | stub | `empty_list` |

### Provider (v1)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/provider` | `provider.list` | ✅ | `get_provider_list` |
| GET | `/provider/auth` | `provider.auth` | ✅ | |
| POST | `/provider/:providerID/oauth/authorize` | `oauth.authorize` | ❌ | 未实现 |
| POST | `/provider/:providerID/oauth/callback` | `oauth.callback` | ❌ | 未实现 |

### Permission (v1)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/permission` | `permission.list` | ✅ | `get_permission_pending` |
| POST | `/permission/:requestID/reply` | `permission.reply` | ✅ | ⚠️ v1/v2 handler 不同 |
| POST | `/session/:sessionID/permissions/:permissionID` | `permission.respond` | stub | `true_value` |

### Question (v1)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/question` | `question.list` | ✅ | `get_question_pending` |
| POST | `/question/:requestID/reply` | `question.reply` | ✅ | ⚠️ v1/v2 handler 不同 |
| POST | `/question/:requestID/reject` | `question.reject` | ✅ | ⚠️ v1/v2 handler 不同 |

### Session (v1)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/session` | `session2.list` | ✅ | ⚠️ v1/v2 handler 不同 |
| POST | `/session` | `session2.create` | ✅ | `create_session` |
| GET | `/session/status` | `session2.status` | ✅ | 全局状态 |
| GET | `/session/:sessionID` | `session2.get` | ✅ | |
| PATCH | `/session/:sessionID` | `session2.update` | ✅ | |
| DELETE | `/session/:sessionID` | `session2.delete` | ✅ | |
| GET | `/session/:sessionID/children` | `session2.children` | ✅ | |
| GET | `/session/:sessionID/todo` | `session2.todo` | ✅ | |
| GET | `/session/:sessionID/diff` | `session2.diff` | ✅ | |
| GET | `/session/:sessionID/message` | `session2.messages` | ✅ | 消息列表 |
| POST | `/session/:sessionID/message` | (prompt) | ✅ | ⚠️ v1/v2 handler 不同 |
| GET | `/session/:sessionID/message/:messageID` | `session2.message` | ✅ | |
| DELETE | `/session/:sessionID/message/:messageID` | `session2.deleteMessage` | ✅ | |
| POST | `/session/:sessionID/prompt_async` | (promptAsync) | ✅ | |
| POST | `/session/:sessionID/command` | `session2.command` | ✅ | ⚠️ v1/v2 handler 不同 |
| POST | `/session/:sessionID/shell` | `session2.shell` | ✅ | ⚠️ v1/v2 handler 不同 |
| POST | `/session/:sessionID/fork` | `session2.fork` | ✅ | |
| POST | `/session/:sessionID/abort` | `session2.abort` | ✅ | **v1: abort** |
| POST | `/session/:sessionID/init` | `session2.init` | ✅ | |
| POST | `/session/:sessionID/share` | `session2.share` | ✅ | |
| DELETE | `/session/:sessionID/share` | `session2.unshare` | ✅ | |
| POST | `/session/:sessionID/summarize` | `session2.summarize` | ✅ | |
| POST | `/session/:sessionID/revert` | `session2.revert` | ❌ | 未实现 |
| POST | `/session/:sessionID/unrevert` | `session2.unrevert` | ❌ | 未实现 |
| PATCH | `/session/:sessionID/message/:messageID/part/:partID` | `part.update` | ✅ | |
| DELETE | `/session/:sessionID/message/:messageID/part/:partID` | `part.delete` | ✅ | |

### PTY (v1)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/pty/shells` | `pty.shells` | ❌ | 未实现 |
| GET | `/pty` | `pty.list` | 501 | |
| POST | `/pty` | `pty.create` | 501 | |
| GET | `/pty/:ptyID` | `pty.get` | 501 | |
| PUT | `/pty/:ptyID` | `pty.update` | 501 | |
| DELETE | `/pty/:ptyID` | `pty.remove` | 501 | |
| POST | `/pty/:ptyID/connect-token` | `pty.connectToken` | 501 | |
| GET | `/pty/:ptyID/connect` | `pty.connect` | 501 | WebSocket |

### Sync (v1)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| POST | `/sync/start` | `sync.start` | ❌ | 未实现 |
| POST | `/sync/replay` | `sync.replay` | ❌ | 未实现 |
| POST | `/sync/steal` | `sync.steal` | ❌ | 未实现 |
| POST | `/sync/history` | `history.list` | ❌ | 未实现 |

### TUI (v1)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| POST | `/tui/append-prompt` | `tui.appendPrompt` | stub | `true_value` |
| POST | `/tui/clear-prompt` | `tui.clearPrompt` | stub | `true_value` |
| POST | `/tui/execute-command` | `tui.executeCommand` | stub | `true_value` |
| POST | `/tui/open-help` | `tui.openHelp` | stub | `true_value` |
| POST | `/tui/open-models` | `tui.openModels` | stub | `true_value` |
| POST | `/tui/open-sessions` | `tui.openSessions` | stub | `true_value` |
| POST | `/tui/open-themes` | `tui.openThemes` | stub | `true_value` |
| POST | `/tui/publish` | `tui.publish` | stub | `true_value` |
| POST | `/tui/select-session` | `tui.selectSession` | stub | `true_value` |
| POST | `/tui/show-toast` | `tui.showToast` | stub | `true_value` |
| POST | `/tui/submit-prompt` | `tui.submitPrompt` | stub | `true_value` |
| GET/POST | `/tui/control/next` | `control.next` | ✅ | `post_tui_control_next` |
| POST | `/tui/control/response` | `control.response` | stub | `true_value` |

### Workspace (v1 experimental)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/experimental/workspace/adapter` | `adapter.list` | ❌ | 未实现 |
| GET | `/experimental/workspace` | `workspace.list` | ❌ | 未实现 |
| POST | `/experimental/workspace` | `workspace.create` | ❌ | 未实现 |
| GET | `/experimental/workspace/sync-list` | `workspace.syncList` | ❌ | 未实现 |
| GET | `/experimental/workspace/status` | `workspace.status` | ❌ | 未实现 |
| DELETE | `/experimental/workspace/:id` | `workspace.remove` | ❌ | 未实现 |
| POST | `/experimental/workspace/warp` | `workspace.warp` | ❌ | 未实现 |

### Experimental

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/experimental/capabilities` | `capabilities.get` | ✅ | |
| GET | `/experimental/console` | `console.get` | ✅ | |
| GET | `/experimental/console/orgs` | `console.listOrgs` | ✅ | |
| POST | `/experimental/console/switch` | `console.switchOrg` | stub | `true_value` |
| GET | `/experimental/tool` | `tool.list` | stub | `empty_list` |
| GET | `/experimental/tool/ids` | `tool.ids` | stub | `empty_list` |
| GET | `/experimental/worktree` | `worktree.list` | ❌ | 未实现 |
| POST | `/experimental/worktree` | `worktree.create` | ❌ | 未实现 |
| DELETE | `/experimental/worktree` | `worktree.remove` | ❌ | 未实现 |
| POST | `/experimental/worktree/reset` | `worktree.reset` | ❌ | 未实现 |
| GET | `/experimental/session` | `session.list` | stub | `empty_list` |
| POST | `/experimental/session/:sessionID/background` | `session.background` | stub | `true_value` |
| GET | `/experimental/resource` | `resource.list` | ✅ | |
| GET | `/experimental/resource/:id` | | ✅ | |
| DELETE | `/experimental/resource/:id` | | ✅ | |
| POST | `/experimental/eval` | | ✅ | |
| POST | `/experimental/project/:projectID/copy/generate-name` | `projectCopy.generateName` | ❌ | 未实现 |
| POST | `/experimental/project/:projectID/copy` | `projectCopy2.create` | ❌ | 未实现 |
| DELETE | `/experimental/project/:projectID/copy` | `projectCopy2.remove` | ❌ | 未实现 |
| POST | `/experimental/project/:projectID/copy/refresh` | `projectCopy2.refresh` | ❌ | 未实现 |

---

## v2 协议路由 (`/api/*`)

### Health

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/api/health` | `health.get` | ✅ | v2 健康检查 |

### Location

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/api/location` | `location.get` | ✅ | `Location.response` 信封 |

### Agent

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/api/agent` | `agent.list` | ✅ | ⚠️ 返回 `{id, mode, ...}` 无 `name` |

### Session (v2)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/api/session` | `session3.list` | ✅ | ⚠️ v1/v2 handler 不同 |
| POST | `/api/session` | `session3.create` | ✅ | |
| GET | `/api/session/active` | `session3.active` | ✅ | |
| GET | `/api/session/:sessionID` | `session3.get` | 🔁 | 同 v1 handler |
| POST | `/api/session/:sessionID/agent` | `session3.switchAgent` | ✅ | **v2 独有** |
| POST | `/api/session/:sessionID/model` | `session3.switchModel` | ❌ | TODO |
| POST | `/api/session/:sessionID/prompt` | `session3.prompt` | ✅ | |
| POST | `/api/session/:sessionID/compact` | `session3.compact` | ❌ | TODO |
| POST | `/api/session/:sessionID/wait` | `session3.wait` | ❌ | TODO |
| POST | `/api/session/:sessionID/revert/stage` | `revert.stage` | ❌ | TODO |
| POST | `/api/session/:sessionID/revert/clear` | `revert.clear` | ❌ | TODO |
| POST | `/api/session/:sessionID/revert/commit` | `revert.commit` | ❌ | TODO |
| GET | `/api/session/:sessionID/context` | `session3.context` | ❌ | TODO |
| GET | `/api/session/:sessionID/history` | `session3.history` | ❌ | TODO |
| GET | `/api/session/:sessionID/event` | `session3.events` | ✅ | v2 会话级 SSE |
| POST | `/api/session/:sessionID/interrupt` | `session3.interrupt` | ✅ | **v2: interrupt** (v1 用 abort) |
| GET | `/api/session/:sessionID/message/:messageID` | `session3.message` | 🔁 | |
| GET | `/api/session/:sessionID/message` | `session3.messages` | ✅ | |

### Permission (v2)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/api/permission/request` | `request.list` | ✅ | ⚠️ v1 用 `/permission` |
| GET | `/api/permission/saved` | `saved.list` | ✅ | |
| DELETE | `/api/permission/saved/:id` | `saved.remove` | stub | `empty_object` |
| POST | `/api/session/:sessionID/permission` | `permission2.create` | ❌ | TODO |
| GET | `/api/session/:sessionID/permission` | `permission2.list` | ❌ | TODO |
| GET | `/api/session/:sessionID/permission/:requestID` | `permission2.get` | ❌ | TODO |
| POST | `/api/session/:sessionID/permission/:requestID/reply` | `permission2.reply` | ❌ | TODO |

### Question (v2)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/api/question/request` | `request2.list` | ✅ | ⚠️ v1 用 `/question` |
| GET | `/api/session/:sessionID/question` | — | ❌ | TODO |
| POST | `/api/session/:sessionID/question/:requestID/reply` | — | ❌ | TODO |
| POST | `/api/session/:sessionID/question/:requestID/reject` | — | ❌ | TODO |

### Model

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/api/model` | `model.list` | ✅ | `Location.response` 信封 |

### Provider (v2)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/api/provider` | `provider2.list` | ✅ | |
| GET | `/api/provider/:providerID` | `provider2.get` | ✅ | |

### Integration (v2)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/api/integration` | `integration.list` | ✅ | |
| GET | `/api/integration/:integrationID` | `integration.get` | stub | |
| POST | `/api/integration/:integrationID/connect/key` | `connect.key` | ❌ | TODO |
| POST | `/api/integration/:integrationID/connect/oauth` | `connect.oauth` | ❌ | TODO |
| GET | `/api/integration/attempt/:attemptID` | `attempt.status` | ❌ | TODO |
| POST | `/api/integration/attempt/:attemptID/complete` | `attempt.complete` | ❌ | TODO |
| DELETE | `/api/integration/attempt/:attemptID` | `attempt.cancel` | ❌ | TODO |

### Credential (v2)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| PATCH | `/api/credential/:credentialID` | `credential.update` | ✅ | |
| DELETE | `/api/credential/:credentialID` | `credential.remove` | ✅ | |

### FileSystem (v2)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/api/fs/read/*` | `fs.read` | ✅ | |
| GET | `/api/fs/list` | `fs.list` | ✅ | |
| GET | `/api/fs/find` | `fs.find` | ✅ | |

### Command / Skill / Reference (v2)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/api/command` | `command2.list` | ✅ | ⚠️ v1/v2 handler 不同 |
| GET | `/api/skill` | `skill.list` | ✅ | |
| GET | `/api/reference` | `reference.list` | ✅ | |

### Event (v2)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/api/event` | `event2.subscribe` | ✅ | v2 SSE，enriched 信封 |

### PTY (v2)

| 方法 | 路径 | SDK 命名空间 | loom 状态 | 备注 |
|------|------|-------------|-----------|------|
| GET | `/api/pty` | `pty2.list` | 501 | |
| POST | `/api/pty` | `pty2.create` | 501 | |
| GET | `/api/pty/:ptyID` | `pty2.get` | 501 | |
| PUT | `/api/pty/:ptyID` | `pty2.update` | 501 | |
| DELETE | `/api/pty/:ptyID` | `pty2.remove` | 501 | |
| POST | `/api/pty/:ptyID/connect-token` | `pty2.connectToken` | 501 | |
| GET | `/api/pty/:ptyID/connect` | `pty2.connect` | 501 | WebSocket |

---

## v1 vs v2 语义差异

### 同名但路径/语义不同

| 功能 | v1 | v2 | 差异 |
|------|----|----|------|
| **中止** | `POST /session/:id/abort` | `POST /api/session/:id/interrupt` | 路径 + handler 均不同 |
| **权限列表** | `GET /permission` | `GET /api/permission/request` | 路径不同 |
| **问题列表** | `GET /question` | `GET /api/question/request` | 路径不同 |
| **状态范围** | `GET /session/status` (全局) | `GET /api/session/:id/status` (会话级) | 作用域不同 |
| **Agent 列表** | `GET /agent` → `{id, name, ...}` | `GET /api/agent` → `{id, mode, ...}` | v2 schema 无 `name` |
| **Command 列表** | `GET /command` → `{name, ...}` | `GET /api/command` → `Location.response` | 信封不同 |
| **Provider 列表** | `GET /provider` → 裸数组 | `GET /api/provider` → `Location.response` | 信封不同 |
| **Session 列表** | `GET /session` → 裸数组 | `GET /api/session` → `Location.response` | 信封不同 |
| **回复信封** | 裸数据 | `{location, data}` 双层包装 | Location.response 是 v2 通用信封 |

### v2 独有（v1 无对应）

| 功能 | v2 路径 | 说明 |
|------|---------|------|
| switchAgent | `POST /api/session/:id/agent` | v1 用 prompt body 传 agent |
| switchModel | `POST /api/session/:id/model` | |
| compact | `POST /api/session/:id/compact` | |
| wait | `POST /api/session/:id/wait` | |
| context | `GET /api/session/:id/context` | |
| history | `GET /api/session/:id/history` | |
| revert stage/clear/commit | `POST /api/session/:id/revert/*` | v1 用单一 `/session/:id/revert` |
| 会话级 SSE | `GET /api/session/:id/event` | 增量回放 |
| Credential | `PATCH/DELETE /api/credential/:id` | |
| Integration | `/api/integration/*` | |
| Location | `GET /api/location` | |
| FileSystem | `/api/fs/*` | |

### v1 独有（v2 无对应）

| 功能 | v1 路径 | 说明 |
|------|---------|------|
| 全局配置 | `GET/PATCH /global/config` | v2 用 `/config` |
| 全局事件 | `GET /global/event` | v2 用 `/api/event` |
| 全局 dispose | `POST /global/dispose` | |
| 全局 upgrade | `POST /global/upgrade` | |
| 实例事件 | `GET /event` | v2 无实例级事件 |
| VCS apply | `POST /vcs/apply` | |
| Provider OAuth | `POST /provider/:id/oauth/*` | |
| Sync | `POST /sync/*` | |
| PTY shells | `GET /pty/shells` | |
| Workspace | `/experimental/workspace/*` | |

---

## 统计

| 指标 | 数量 |
|------|------|
| opencode 定义端点总数 | ~180 |
| v1 实例路由 | ~105 |
| v2 协议路由 | ~55 |
| v1+v2 重叠（同一功能两套路径） | ~40 |
| loom-server ✅ 已实现 | ~100 |
| loom-server ❌ 未实现/TODO | ~45 |
| loom-server stub/501 | ~25 |
| loom-server ⚠️ v1/v2 handler 不同 | ~12 |

## 源码引用

| 来源 | 路径 |
|------|------|
| opencode v1 route groups | `packages/opencode/src/server/routes/instance/httpapi/groups/*.ts` |
| opencode v2 protocol groups | `packages/protocol/src/groups/*.ts` |
| opencode API assembly | `packages/opencode/src/server/routes/instance/httpapi/api.ts` |
| opencode server mount | `packages/opencode/src/server/routes/instance/httpapi/server.ts` |
| opencode SDK client gen | `packages/sdk/js/src/v2/gen/sdk.gen.ts` |
| opencode SDK client wrapper | `packages/sdk/js/src/v2/client.ts` |
| TUI bootstrap calls | `packages/tui/src/context/sync.tsx` |
| loom-server route registry | `apps/server/src/routes.rs` |
| loom-server SSE | `apps/server/src/sse.rs` |
| loom-server auth | `apps/server/src/auth.rs` |
| loom-server Location envelope | `apps/server/src/location.rs` |
