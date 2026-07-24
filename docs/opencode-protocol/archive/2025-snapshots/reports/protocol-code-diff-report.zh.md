# opencode vs loom-server：深度行为代码对比报告

> 生成日期：2025-08-19。数据来源：8 份分析文件 + FP/FN 对抗性审查。
>
> 方法：8 个并行 minimax-m3 agent 逐行阅读 opencode（TS）和 loom-server（Rust）两端的 handler 代码，拆分为逻辑块（输入校验、鉴权、业务逻辑、错误处理、响应构建、序列化）。两名对抗性审查者（FP-hunter + FN-hunter）交叉验证所有发现。

---

## 概况

| 指标 | 数值 |
|------|------|
| 分析端点数 | 108 |
| 分析逻辑块数 | 99 |
| opencode 阅读行数 | ~4,200（21 group 文件 + 18 protocol 文件 + types.gen.ts + sdk.gen.ts） |
| loom 阅读行数 | ~3,800（26 handler 文件 + routes.rs + sse.rs + state.rs + location.rs） |
| **一致** | **48（44%）** |
| 基本一致（有保留） | 18（17%） |
| 存在分歧（逻辑/字段/错误） | 30（28%） |
| 占位/501 | 22（20%） |
| loom 缺失 | 8（7%） |
| loom 独有 | 6（6%） |
| **一致性** | **44%** |

### 按严重程度分布

| 严重程度 | 数量 |
|----------|------|
| 严重（Critical） | 16 |
| 重要（Major） | 28 |
| 一般（Minor） | 32 |
| 细微（Nit） | 4 |
| 无（一致） | 48 |

---

## 对抗性审查影响

| 指标 | 数值 |
|------|------|
| 移除的误报（FP） | 1 |
| 追加的漏报（FN） | 8 |
| 严重级别上调 | 4 |
| **净变化** | **+7** |

**误报推翻**：`DELETE /api/session/:id/message/:messageID` — 分析标记为"loom 独有"，但 OC v1 `session.ts:409` 实际定义了此端点。不过返回类型仍有问题（{ok:bool} vs boolean）。

**严重级别上调**：
- `POST /api/session/:id/interrupt`：重要 → **严重**（期望 204，返回 JSON）
- `POST /session/:id/prompt_async`：重要 → **严重**（期望 204，返回 JSON）
- `POST /session/:id/abort`：一般 → **重要**（期望 boolean，返回对象）
- `GET /find/file`：重要 → **严重**（期望 string[]，返回对象）

**关键漏报发现**：
- Session.Model 字段名：OC 用 `id`，loom 用 `modelID`
- Session.path 类型：OC 是 string，loom 是 `{cwd,root}` 对象
- ~40 种 `session.next.*` 事件类型完全缺失

---

## 严重分歧

### 1. 缺失 `session.next.*` 事件族（~40 种）

| | 详情 |
|---|---|
| **端点** | `GET /api/event` + `GET /api/session/:id/event`（SSE） |
| **OC** | 发射约 40 种 `session.next.*` 事件用于实时 agent 进度：`step.started/ended/failed`、`text.started/ended/delta`、`tool.called/success/failed/input.started/ended/delta/progress`、`reasoning.started/ended/delta`、`compaction.started/ended/delta`、`shell.started/ended`、`agent.switched`、`model.switched`、`context.updated`、`retried`、`revert.staged/cleared/committed`、`moved`、`prompt.admitted`、`synthetic` |
| **Loom** | 未发射任何 `session.next.*` 事件。仅有：`session.created/updated/deleted/status/error`、`message.updated/part.updated/part.delta`、`permission.*`、`question.*` |
| **证据** | OC：`packages/schema/src/event.ts` 约 40 个定义。Loom：`grep 'session.next'` = 0 匹配 |
| **影响** | TUI 会话详情页无法显示 agent 进度（无步骤指示器、文本流、工具调用状态、推理展示）。用户只看到静态会话直到完成。 |
| **修复** | 在 `translator.rs` 中将 loom agent runner 事件映射到 `session.next.*` 类型 |

### 2. Session.Model 字段名不匹配

| | 详情 |
|---|---|
| **端点** | 所有带 `model` 字段的 session 响应 |
| **OC** | `Model = {id: ModelV2.ID, providerID: ProviderV2.ID, variant?}` — `session.ts:216-220` |
| **Loom** | `ModelInfo = {providerID, modelID, variant}` — `state.rs:263-270` |
| **影响** | TUI 读取 `session.model.id` — 得到 `undefined`。每个会话的模型显示都有问题。 |
| **修复** | 在 `ModelInfo` 的 serde 属性中将 `modelID` 重命名为 `id` |

### 3. Session.path 类型不匹配

| | 详情 |
|---|---|
| **端点** | 所有带 `path` 字段的 session 响应 |
| **OC** | `path: optional(Schema.String)` — 相对路径，如 `"packages/web/src"` — `session.ts:230` |
| **Loom** | `path: Option<PathInfo>`，其中 `PathInfo = {cwd: String, root: String}` — `state.rs:203,231-234` |
| **影响** | TUI 将 path 作为文本渲染 — 得到 `[object Object]`。会话路径显示损坏。 |
| **修复** | 将 `PathInfo` 改为 `Option<String>`，通过 `path.relative(root, cwd)` 计算 |

### 4. GET /file — 语义不匹配

| | 详情 |
|---|---|
| **OC** | 目录列表：`GET /file?path=...` → `LegacyEntry[] {name, path, absolute, type, ignored}` — `file.ts:138-147` |
| **Loom** | 文件内容读取：`GET /file?path=...` → `{content: string, path: string}` — `mcp_pty_file.rs:281-298` |
| **影响** | TUI 文件浏览器收到文件内容而非目录列表。完全的语义不匹配。 |

### 5. GET /find — 语义不匹配

| | 详情 |
|---|---|
| **OC** | 内容搜索（ripgrep）：`GET /find?pattern=...` → `LegacyMatch[] {path, lines, line_number, absolute_offset, submatches}` — `file.ts:108-117` |
| **Loom** | 文件名搜索：`GET /find?pattern=...` → `string[]`（文件路径） — `mcp_pty_file.rs:367-375` |
| **影响** | TUI 文本搜索返回文件名匹配而非内容匹配。 |

### 6. GET /vcs/status — 响应结构

| | 详情 |
|---|---|
| **OC** | `Vcs.FileStatus[] {file, additions, deletions, status}` — 每文件带行数统计 — `vcs.ts:258-264` |
| **Loom** | `{dirty, branch, ahead, behind, modified[], staged[], untracked[]}` — 汇总对象 — `vcs_extra.rs:259-267` |
| **影响** | TUI 期望按文件的 diff 统计。得到的是分支状态汇总。 |

### 7. GET /vcs/diff — 响应结构

| | 详情 |
|---|---|
| **OC** | `Vcs.FileDiff[] {file, patch?, additions, deletions, status?}` — 结构化按文件 — `vcs.ts:246-256` |
| **Loom** | `{diff, unstaged, staged}` — 原始拼接文本 — `vcs_extra.rs:72-96` |
| **影响** | TUI 期望带行数统计的结构化 diff。得到原始文本。 |

### 8. GET /vcs/diff/raw — Content-Type

| | 详情 |
|---|---|
| **OC** | 原始文本，`Content-Type: text/x-diff; charset=utf-8` — `instance.ts:117` |
| **Loom** | JSON `{diff, staged}`，`Content-Type: application/json` — `vcs_extra.rs:100-123` |
| **影响** | 期望原始 patch 文本的消费者收到 JSON。 |

### 9. POST /api/session/:id/interrupt — 返回类型

| | 详情 |
|---|---|
| **OC** | `HttpApiSchema.NoContent` → 204 空响应体 |
| **Loom** | `Json({ok:true, cancelled:bool})` → 200 JSON 响应体 — `session.rs:816-821` |
| **影响** | SDK 期望空 204。JSON 响应体会导致解析错误。 |

### 10. POST /session/:id/prompt_async — 返回类型

| | 详情 |
|---|---|
| **OC** | `HttpApiSchema.NoContent` → 204 — `session.ts:333` |
| **Loom** | `{ok: true}` → 200 — `session.rs:410` |
| **影响** | 同上。 |

### 11. GET /find/file — 响应结构

| | 详情 |
|---|---|
| **OC** | `Schema.Array(Schema.String)` — 纯 `string[]` — `file.ts:118-127` |
| **Loom** | `{data: [{name, type, size}]}` — 包含 Entry 对象的外层包装 — `mcp_pty_file.rs:418-432` |
| **影响** | 期望 `string[]` 的消费者收到带数组的对象。类型不匹配导致崩溃。 |

### 12. GET /session/:id/todo — 占位 + 信封不匹配

| | 详情 |
|---|---|
| **OC** | `Todo.Info[]` — 纯数组，从 assistant 消息中解析 |
| **Loom** | `{sessionID, todos: []}` — 占位对象，信封错误 — `session.rs:1036-1046` |
| **影响** | Todo 列表永远为空 + 响应结构错误。另有：双 handler 冲突 bug（messages.rs 返回 `[]`，session.rs 返回 `{sessionID, todos:[]}`）。 |

### 13. GET /session/:id/diff — 占位 + 信封不匹配

| | 详情 |
|---|---|
| **OC** | `Snapshot.FileDiff[]` — 纯数组 |
| **Loom** | `{sessionID, diff: []}` — 占位，信封错误 — `session.rs:1049-1059` |
| **影响** | Diff 列表永远为空 + 结构错误。 |

### 14. Assistant 消息 Schema 不匹配

| | 详情 |
|---|---|
| **OC** | `SessionV1.Assistant`：`modelID`、`providerID`、`mode`、`path`、`cost`、`tokens` 均为**必填**。另有 `error?`、`structured?`、`variant?`、`summary?` — `schema/v1/session.ts:453-485` |
| **Loom** | `MessageInfo`：上述字段全为 `Option`（可选）。缺失：`error`、`structured`、`variant`、`summary` — `state.rs:286-315` |
| **影响** | 严格的 schema 校验器会拒绝 loom 的 assistant 消息。缺失字段导致 TUI 渲染空白。 |

### 15. GET /config — 大面积字段缺失

| | 详情 |
|---|---|
| **OC** | `ConfigV1.Info`：约 30 个字段（`$schema`、`shell`、`logLevel`、`server`、`agent`、`instructions`、`username`、`default_agent` 等） |
| **Loom** | `AppState::config`：4 个字段（`theme`、`model`、`provider`、`providers`） — `bootstrap.rs:86-89` |
| **影响** | TUI 配置页大部分字段显示为空/缺失。 |

### 16. DELETE /session/:id — 返回类型

| | 详情 |
|---|---|
| **OC** | `Schema.Boolean` → `{success: true}` JSON — `session.ts:218` |
| **Loom** | `StatusCode::NO_CONTENT` → 204 空响应体 — `session.rs:124` |
| **影响** | TUI 对响应调用 `.json()` — 空响应体导致解析错误。 |

---

## 重要分歧

| # | 端点 | 问题 | OC | Loom |
|---|------|------|----|----|
| 1 | User 消息（全部） | 缺失 3 个字段 | `format?`、`system?`、`tools?: Record<string,boolean>` | `MessageInfo` 中无此字段 |
| 2 | `GET /agent` | 多余 + 缺失字段 | `Agent.Info` 无 `id` | Loom 添加了 `id`、`permissions`、`request`；缺失 `native`、`topP`、`temperature`、`color`、`model`、`variant`、`prompt`、`steps` |
| 3 | `GET /provider` | `connected` 类型 | `connected: string[]`（ID 数组） | `connected: Provider.Info[]`（对象数组） |
| 4 | `GET /path` | 多余字段 | `PathInfo {home, state, config, worktree, directory}` | 添加了 `cwd`、`root`、`cache` |
| 5 | `POST /session` | 忽略 body 字段 | 接受 `model`、`parentID`、`workspaceID` | 三者全部忽略 |
| 6 | `PATCH /session/:id` | 字段集不相交 | 修改 `title`、`metadata`、`permission`、`time.archived` | 修改 `agent`、`workspaceID`、`parentID`、`directory` |
| 7 | `POST /session/:id/fork` | 不支持部分 fork | 支持 `messageID` 进行部分 fork | 拷贝全部消息 |
| 8 | `POST /session/:id/command` | 无模板解析 | 完整的 `$1/$2/$ARGUMENTS` 占位符系统 | 直接拼接 `/{command} {args}` 作为原始文本 |
| 9 | `POST /session/:id/shell` | 无繁忙检查 | 运行中返回 `SessionBusyError`（409） | 不检查 — 直接运行 |
| 10 | `GET /session` | 无过滤 | 支持 `scope`、`path`、`roots`、`start`、`search`、`limit` | 返回全部会话 |
| 11 | `GET /permission`（v1） | 字段结构错误 | `PermissionV1.Request {id, sessionID, permission, patterns, metadata, always, tool?}` | 返回 v2 结构 `{action, resources, save, source}` |
| 12 | `POST /permission/:id/reply` | 返回类型 | `Schema.Boolean` | 返回 request 对象 |
| 13 | `POST /question/:id/reply` | 返回类型 | `Schema.Boolean` | 返回 `{ok, requestID, answers}` |
| 14 | `POST /session/:id/abort` | 返回类型 | `Schema.Boolean` | 返回 `{ok:true, cancelled:bool}` |
| 15 | `GET /vcs` | 硬编码分支 | 实际执行 `git rev-parse --abbrev-ref HEAD` | 硬编码 `'main'`，缺失 `default_branch` |
| 16 | `GET /file/content` | 缺失字段 | `LegacyContent {type, content, diff?, patch?, encoding?, mimeType?}` | 仅 `{content, path}` |
| 17 | `GET /api/session/active` | 无过滤 | 仅返回有活跃 agent 循环的会话 | 返回全部会话 |
| 18 | `GET /api/provider` | 信封错误 | `Location.response(Provider.Info[])` | 纯 `{all, default, connected}` |
| 19 | `POST /api/session/:id/prompt` | 文件/agent 被丢弃 | `PromptInput.Prompt {text, files?, agents?}` | 忽略 `files` 和 `agents` |
| 20 | `POST /session/:id/share` | 假 URL | 真实的云端分享链接 | `https://example.com/share/{id}` |
| 21 | `DELETE /session/:id/share` | 返回类型 | `Session.Info` | 204 No Content |
| 22 | `GET /session/:id/children` | 错误处理 | `ApiNotFoundError` 404 | 返回空数组 `[]` |
| 23 | `DELETE .../part/:partID` | 返回类型 | `Schema.Boolean` | `{ok: bool}` |
| 24 | `PATCH .../part/:partID` | 自动创建 | 未找到时返回 404 | 消息存在时自动创建新 part |
| 25 | Summary 子 schema | 缺失字段 | `{additions, deletions, files, diffs?}` | 无 `diffs` 字段 |
| 26 | MCP 管理 | 全部占位 | 真实 MCP 服务器生命周期 | POST/PATCH/DELETE/auth 全是 `true_value` |
| 27 | `GET /skill` | 空占位 | 从配置读取真实技能列表 | `location_response(json!([]))` |
| 28 | `POST /session/:id/init` | 占位 | 创建 AGENTS.md | `{ok:true}` 空操作 |
| 29 | `POST /session/:id/summarize` | 占位 | AI 摘要 | `{ok:true, summary:''}` 空操作 |
| 30 | Agent 循环 | 缺失子系统 | 上下文压缩、权限系统、MCP/LSP 集成 | 基础 LLM 工具调用循环 |

---

## 字段级问题

| 端点 | 字段 | OC 类型 | Loom 类型 | 问题 |
|------|------|---------|-----------|------|
| Session（全部） | `model.id` | `ModelV2.ID`（字段名：`id`） | `modelID` | **JSON key 名错误** |
| Session（全部） | `path` | `string`（相对路径） | `{cwd, root}` 对象 | **类型错误** |
| Session（全部） | `summary.diffs` | `Snapshot.FileDiff[]?` | 缺失 | 缺失可选字段 |
| `/agent` | `[].id` | 缺失 | `string` | OC 中不存在的多余字段 |
| `/agent` | `[].name` | `string` | 缺失（v2）/ 存在（v1） | v2 正确省略 |
| `/agent` | `[].native` | `boolean?` | 缺失 | 缺失可选字段 |
| `/agent` | `[].topP` | `Finite?` | 缺失 | 缺失可选字段 |
| `/agent` | `[].temperature` | `Finite?` | 缺失 | 缺失可选字段 |
| `/agent` | `[].color` | `string?` | 缺失 | 缺失可选字段 |
| `/agent` | `[].model` | `struct?` | 缺失 | 缺失可选字段 |
| `/agent` | `[].steps` | `Finite?` | 缺失 | 缺失可选字段 |
| `/config` | `$schema` | `string?` | 缺失 | 缺失字段 |
| `/config` | `shell` | `string?` | 缺失 | 缺失字段 |
| `/config` | `logLevel` | `enum?` | 缺失 | 缺失字段 |
| `/config` | `agent` | `struct?` | 缺失 | 缺失字段 |
| `/config` | `instructions` | `array?` | 缺失 | 缺失字段 |
| `/config` | `username` | `string?` | 缺失 | 缺失字段 |
| `/provider` | `connected` | `string[]` | `Provider.Info[]` | **元素类型错误** |
| `/path` | `cwd` | 缺失 | `string` | 多余字段 |
| `/path` | `root` | 缺失 | `string` | 多余字段 |
| `/path` | `cache` | 缺失 | `string` | 多余字段 |
| User 消息 | `format` | `Format?` | 缺失 | 缺失字段 |
| User 消息 | `system` | `string?` | 缺失 | 缺失字段 |
| User 消息 | `tools` | `Record<string,boolean>?` | 缺失 | 缺失字段 |
| Assistant 消息 | `modelID` | `Model.ID`（必填） | `Option<String>` | 应为必填 |
| Assistant 消息 | `providerID` | `Provider.ID`（必填） | `Option<String>` | 应为必填 |
| Assistant 消息 | `mode` | `string`（必填） | `Option<String>` | 应为必填 |
| Assistant 消息 | `path` | `{cwd, root}`（必填） | `Option<Value>` | 应为必填 |
| Assistant 消息 | `cost` | `Finite`（必填） | `Option<f64>` | 应为必填 |
| Assistant 消息 | `tokens` | `struct`（必填） | `Option<Value>` | 应为必填 |
| Assistant 消息 | `error` | `AssistantError?` | 缺失 | 缺失字段 |
| Assistant 消息 | `structured` | `Any?` | 缺失 | 缺失字段 |
| Assistant 消息 | `variant` | `string?` | 缺失 | 缺失字段 |
| Assistant 消息 | `summary` | `boolean?` | 缺失 | 缺失字段 |
| `/file/content` | `type` | `'text'\|'binary'` | 缺失 | 缺失字段 |
| `/file/content` | `diff` | `string?` | 缺失 | 缺失字段 |
| `/file/content` | `patch` | `struct?` | 缺失 | 缺失字段 |
| `/file/content` | `encoding` | `'base64'?` | 缺失 | 缺失字段 |
| `/file/content` | `mimeType` | `string?` | 缺失 | 缺失字段 |

---

## 错误处理差异

| 端点 | OC 错误 | Loom 错误 | 影响 |
|------|---------|-----------|------|
| `GET /session/:id` | `ApiNotFoundError` {_tag, message} JSON 404 | 纯 `StatusCode::NOT_FOUND` 404 | 无 JSON body — TUI 无法解析错误 |
| `GET /session/:id/children` | `ApiNotFoundError` 404 | 空数组 `[]`（200） | 不报错 — 静默成功 |
| `GET /session/:id/message` | `ApiNotFoundError` 404 | 空数组 `[]`（200） | 不报错 — 静默成功 |
| `POST /session/:id/shell` | `SessionBusyError` 409 | 不检查 | 可能并发执行 |
| `POST /api/session/:id/prompt` | `ConflictError` 409 | 不检查 | 可能重复提交 |
| `GET /api/session/:id/message` | `InvalidCursorError` 400 | 未实现 | 坏游标被静默忽略 |
| `POST /api/integration/:id/attempt/complete` | `InvalidRequestError` 400（code 不正确） | 400（诚实 — 无 OAuth 提供商） | 行为正确，原因不同 |
| `POST /session/:id/abort` | `BadRequest` 400 | 无 — 总是返回 | 无校验 |
| `DELETE /session/:id` | `ApiNotFoundError` 404 | 404（纯状态码） | 无 JSON body |

**规律**：Loom 系统性地返回纯 HTTP 状态码（如 `StatusCode::NOT_FOUND`）而不带 JSON body，而 OC 返回带 `_tag` 区分符的结构化错误对象。TUI 的错误处理依赖 `_tag` 来区分错误类型。

---

## 完整端点矩阵

### v1 端点（无 `/api/` 前缀，共 73 个）

| 方法 | 路径 | 判定 | 关键问题 |
|------|------|------|----------|
| GET | `/global/health` | 一致 | — |
| GET | `/global/event` | 一致 | v1 SSE 信封正确 |
| GET | `/global/config` | 一致 | — |
| PATCH | `/global/config` | 一致 | — |
| GET | `/config` | **严重** | 4 个字段 vs 30+ |
| PATCH | `/config` | 一致 | — |
| GET | `/config/providers` | 基本一致 | `source`/`env` 硬编码 |
| GET | `/agent` | **重要** | 多余 `id`，缺失 8 个可选字段 |
| GET | `/skill` | 占位 | 空数组 |
| GET | `/command` | 一般 | 硬编码 2 个命令 |
| GET | `/path` | **重要** | 3 个多余字段 |
| GET | `/lsp` | 基本一致 | 空数组（诚实） |
| GET | `/formatter` | 基本一致 | 空数组（诚实） |
| GET | `/provider` | **重要** | `connected` 类型错误 |
| GET | `/provider/auth` | 一致 | — |
| GET | `/vcs` | **重要** | 硬编码分支 |
| GET | `/vcs/status` | **严重** | 响应结构错误 |
| GET | `/vcs/diff` | **严重** | 响应结构错误 |
| GET | `/vcs/diff/raw` | **严重** | Content-Type 错误 |
| POST | `/vcs/apply` | 缺失 | 端点未注册 |
| GET | `/file` | **严重** | 语义不匹配 |
| PUT | `/file` | loom 独有 | — |
| GET | `/file/content` | **重要** | 缺失 5 个字段 |
| GET | `/file/status` | 占位 | 空数组 |
| GET | `/find` | **严重** | 语义不匹配 |
| POST | `/find` | loom 独有 | — |
| GET | `/find/file` | **严重** | 结构 + 元素类型错误 |
| GET | `/find/symbol` | 占位 | 空数组 |
| GET | `/mcp` | 基本一致 | 空服务器列表（诚实） |
| POST | `/mcp` | 占位 | true_value |
| PATCH | `/mcp` | 占位 | true_value |
| POST | `/mcp/:name/auth` | 占位 | true_value |
| DELETE | `/mcp/:name/auth` | 占位 | true_value |
| POST | `/mcp/:name/auth/callback` | 占位 | true_value |
| POST | `/mcp/:name/auth/authenticate` | 缺失 | 未注册 |
| POST | `/mcp/:name/connect` | 缺失 | 未注册 |
| POST | `/mcp/:name/disconnect` | 缺失 | 未注册 |
| GET | `/project` | 基本一致 | v1/v2 handler 不同 |
| GET | `/project/current` | 基本一致 | — |
| GET | `/permission` | **重要** | v1 端点返回 v2 结构 |
| POST | `/permission/:id/reply` | **重要** | 返回对象，OC 期望 boolean |
| GET | `/question` | 基本一致 | — |
| POST | `/question/:id/reply` | **重要** | 返回对象，OC 期望 boolean |
| POST | `/question/:id/reject` | 一致 | 正确 boolean true |
| GET | `/session` | **重要** | 缺失查询过滤 + Model/Path 问题 |
| POST | `/session` | **重要** | 忽略 model/parentID/workspaceID |
| GET | `/session/status` | 基本一致 | — |
| GET | `/session/:id` | **严重** | Model.id/path 类型不匹配 |
| PATCH | `/session/:id` | **重要** | 字段集不相交 |
| DELETE | `/session/:id` | **严重** | 204 vs boolean JSON |
| GET | `/session/:id/children` | **重要** | 返回 [] 而非 404 |
| GET | `/session/:id/todo` | **严重** | 占位 + 信封不匹配 |
| GET | `/session/:id/diff` | **严重** | 占位 + 信封不匹配 |
| POST | `/session/:id/message` | **重要** | 缺失 agent 循环功能 + 消息字段 |
| GET | `/session/:id/message` | 基本一致 | 返回 [] 而非 404 |
| GET | `/session/:id/message/:msgID` | 一致 | — |
| POST | `/session/:id/prompt_async` | **严重** | 200+JSON vs 204 |
| POST | `/session/:id/command` | **重要** | 无模板解析 |
| POST | `/session/:id/shell` | **重要** | 无繁忙检查 |
| POST | `/session/:id/fork` | **重要** | 不支持部分 fork |
| POST | `/session/:id/abort` | **重要** | 对象 vs boolean |
| POST | `/session/:id/init` | **重要** | 占位 — 无 AGENTS.md |
| POST | `/session/:id/share` | **重要** | 假分享 URL |
| DELETE | `/session/:id/share` | **重要** | 204 vs Session.Info |
| POST | `/session/:id/summarize` | **重要** | 占位 — 无摘要 |
| PATCH | `.../part/:partID` | **重要** | 缺失时自动创建 |
| DELETE | `.../part/:partID` | **重要** | {ok:bool} vs boolean |
| GET | `/git/check` | loom 独有 | — |
| POST | `/git/stage` | loom 独有 | — |
| POST | `/git/unstage` | loom 独有 | — |
| POST | `/git/commit` | loom 独有 | — |
| GET | `/git/log` | loom 独有 | — |
| GET | `/git/branches` | loom 独有 | — |

### v2 端点（`/api/` 前缀，共 56 个）

| 方法 | 路径 | 判定 | 关键问题 |
|------|------|------|----------|
| GET | `/api/agent` | 基本一致 | v2 schema 正确，硬编码 |
| GET | `/api/skill` | 占位 | 空数组 |
| GET | `/api/command` | 一般 | 硬编码，信封正确 |
| GET | `/api/path` | **重要** | 同 /path |
| GET | `/api/model` | 一致 | models.dev 集成 |
| GET | `/api/location` | 一致 | 纯 Location.Info 正确 |
| GET | `/api/provider` | **重要** | 无 Location.response 信封 |
| GET | `/api/provider/:id` | 一致 | — |
| GET | `/api/fs/read/*` | 一致 | 原始字节，路径遍历保护 |
| GET | `/api/fs/list` | 一致 | Location.response 正确 |
| GET | `/api/fs/find` | 一致 | ripgrep + walk 回退 |
| POST | `/api/fs/write` | loom 独有 | — |
| POST | `/api/fs/delete` | loom 独有 | — |
| GET | `/api/mcp` | 一般 | 多余 `{data:}` 包装 |
| GET | `/api/permission/request` | 一致 | 信封正确 |
| POST | `/api/session/:id/permission` | 一致 | 正确的 'ask' 默认值 |
| GET | `/api/session/:id/permission` | 一致 | — |
| GET | `/api/session/:id/permission/:reqID` | 一致 | 错误标签正确 |
| POST | `/api/session/:id/permission/:reqID/reply` | 一致 | 正确 204 |
| GET | `/api/question/request` | 一致 | — |
| GET | `/api/session/:id/question` | 一致 | — |
| POST | `/api/session/:id/question/:reqID/reply` | 一致 | 正确 204 |
| POST | `/api/session/:id/question/:reqID/reject` | 一致 | 正确 204 |
| POST | `/api/session/:id/prompt` | **重要** | files/agents 被丢弃 |
| POST | `/api/session/:id/agent` | 一致 | 正确 204 |
| POST | `/api/session/:id/interrupt` | **严重** | 200+JSON vs 204 |
| POST | `/api/session/:id/model` | 一致 | 正确 204 |
| POST | `/api/session/:id/compact` | 占位 | 诚实的 501 |
| POST | `/api/session/:id/wait` | 一致 | 正确阻塞 + 超时 |
| GET | `/api/session/:id/context` | 一致 | 正确 {data:[{info,parts}]} |
| GET | `/api/session/:id/history` | 占位 | 诚实的 501 |
| POST | `/api/session/:id/revert/stage` | 占位 | 诚实的 501 |
| POST | `/api/session/:id/revert/clear` | 占位 | 诚实的 501 |
| POST | `/api/session/:id/revert/commit` | 占位 | 诚实的 501 |
| GET | `/api/session` | 基本一致 | 游标分页可用 |
| GET | `/api/session/active` | **重要** | 返回全部，未过滤 |
| GET | `/api/session/:id/event` | 一致 | 正确游标回放 |
| GET | `/api/event` | **严重** | 缺失 40 种 session.next.* 事件 |
| GET | `/api/health` | 一致 | — |
| PATCH | `/api/credential/:id` | 一致 | 正确 204 |
| DELETE | `/api/credential/:id` | 一致 | 幂等 204 |
| GET | `/api/integration` | 一致 | 从凭据生成真实目录 |
| GET | `/api/integration/:id` | 一致 | 正确的 UndefinedOr |
| POST | `/api/integration/:id/connect/key` | 一致 | 真实密钥存储 |
| POST | `/api/integration/:id/connect/oauth` | 基本一致 | 无 OAuth 提供商 |
| GET | `/api/integration/attempt/:id` | 一致 | 正确状态 + 惰性过期 |
| POST | `/api/integration/attempt/:id/complete` | **重要** | 诚实的 400（无 OAuth） |
| DELETE | `/api/integration/attempt/:id` | 一致 | 幂等 204 |
| GET | `/api/instance` | loom 独有 | — |
| GET | `/api/pty` | 占位 | 空，无 Location 信封 |
| POST | `/api/pty` | 占位 | 501 |
| GET | `/api/pty/:id` | 占位 | 501 |
| PUT | `/api/pty/:id` | 占位 | 501 |
| DELETE | `/api/pty/:id` | 占位 | 501 |
| POST | `/api/pty/:id/connect-token` | 占位 | 501 |
| GET | `/api/pty/:id/connect` | 占位 | 501（WebSocket） |

### 按版本统计

| 判定 | v1（无 `/api/`） | v2（`/api/`） |
|------|:---:|:---:|
| 一致 | 8（11%） | 28（50%） |
| 基本一致 | 9（12%） | 3（5%） |
| 一般 | 1（1%） | 2（4%） |
| 重要 | 23（32%） | 5（9%） |
| 严重 | 12（16%） | 2（4%） |
| 占位 | 8（11%） | 13（23%） |
| 缺失 | 4（5%） | 0 |
| loom 独有 | 8（11%） | 3（5%） |
| **合计** | **73** | **56** |

> v2 一致率（含"基本一致"）55%，v1 仅 23%。严重问题集中在 v1（12 vs 2）。

---

## 系统性规律

### 规律 1：返回类型不匹配（boolean vs 对象 / 204 vs 200）
影响 12+ 个端点。OC v1 使用 `Schema.Boolean` 表示成功 → 期望 `{success: true}` 或原始 `true`。OC v2 使用 `HttpApiSchema.NoContent` → 期望 204 空。Loom 返回 `{ok: true, ...}` 对象 + 200 状态码。

**修复**：审查所有变更端点，映射到正确的 OC 返回类型。

### 规律 2：缺失查询参数支持
`GET /session`（search/limit/scope）、`GET /vcs/diff`（mode/context）、`GET /find/file`（dirs/type/limit）— loom 忽略所有查询参数。

### 规律 3：占位端点的错误结构
Todo、diff、skill、reference、file/status、find/symbol — 都返回空但用了错误的结构（包装 vs 纯数组）。

### 规律 4：v2 端点使用 v1 handler
`/api/provider`、`/api/mcp` — 双注册到返回 v2 协议错误结构的 v1 handler。

### 规律 5：缺失错误响应体
Loom 返回纯 `StatusCode` 不带 JSON。OC 返回 `{_tag, message, ...}` 结构化错误。TUI 错误处理依赖 `_tag` 区分符。

### 规律 6：缺失 Agent 循环子系统
上下文压缩、权限执行、MCP 工具集成、LSP 诊断、插件钩子 — loom 的 agent runner 中全部缺失。

---

## 修复建议（按优先级排序）

1. **修复 Session.Model 字段名**（`modelID` → `id`） — 1 行 serde rename
2. **修复 Session.path 类型**（对象 → 字符串） — 结构性变更
3. **实现 session.next.* 事件** — translator.rs 事件映射
4. **修复返回类型**（v1 变更用 boolean，v2 用 204） — 审查并修复每个 handler
5. **修复 /file 和 /find 语义不匹配** — 重写为目录列表 + 内容搜索
6. **修复 /vcs/status 和 /vcs/diff** — 将 git 输出解析为结构化的每文件对象
7. **修复 /config 字段缺失** — 在 config 结构体中添加缺失的 26 个字段
8. **修复错误响应体** — 为所有 404/400 响应添加 `_tag` 结构化错误 JSON
9. **修复 /agent 字段缺失** — 添加缺失的可选字段或直接使用 OC Agent.Info
10. **修复 /session/:id/todo 和 /diff** — 实现真实提取或至少修正信封结构
