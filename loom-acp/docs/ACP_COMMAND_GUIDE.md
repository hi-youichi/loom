# Loom ACP Protocol Command Reference

> 本文档描述 `loom-acp` 实现的所有 ACP 协议方法（JSON-RPC commands），包括请求/响应格式、Loom 映射和实现细节。
>
> 依赖版本: `agent-client-protocol = "0.11.1"` | 协议版本: ACP v1

## 目录

- [请求方法（Client → Agent）](#请求方法)
  - [initialize](#initialize)
  - [authenticate](#authenticate)
  - [session/new](#sessionnew)
  - [session/prompt](#sessionprompt)
  - [session/fork](#sessionfork)
  - [session/load](#sessionload)
  - [session/list](#sessionlist)
  - [setSessionConfigOption](#setsessionconfigoption)
  - [setSessionMode](#setsessionmode)
  - [setSessionModel](#setsessionmodel)
- [通知方法（Client → Agent，无响应）](#通知方法)
  - [session/cancel](#sessioncancel)
- [Agent → Client 方法](#agent--client-方法)
  - [session/update](#sessionupdate-notification)
  - [session/request_permission](#sessionrequest_permission)
  - [fs/read_text_file](#fsread_text_file)
  - [fs/write_text_file](#fswrite_text_file)
  - [terminal/create](#terminalcreate)
  - [terminal/output](#terminaloutput)
  - [terminal/wait_for_exit](#terminalwait_for_exit)
  - [terminal/kill](#terminalkill)
  - [terminal/release](#terminalrelease)
- [数据类型参考](#数据类型参考)
- [未实现的 ACP 方法](#未实现的-acp-方法)

---

## 请求方法

### initialize

**何时**: 连接建立后，调用其他方法前。仅调用一次。

**JSON-RPC Method**: `initialize`

**Request**:
```json
{
  "protocol_version": "1",
  "client_capabilities": {
    "fs": { "read_text_file": true, "write_text_file": true },
    "terminal": true
  },
  "implementation": { "name": "Zed", "version": "0.180.0" }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `protocol_version` | string | ✅ | 协议版本，Loom 返回 `"1"` |
| `client_capabilities` | object | ❌ | 客户端能力声明 |
| `implementation` | object | ❌ | 客户端实现信息 |

**Response**:
```json
{
  "protocol_version": "1",
  "agent_info": { "name": "Loom", "version": "0.1.0" },
  "agent_capabilities": {
    "load_session": true,
    "session_capabilities": {
      "list": {},
      "fork": {}
    },
    "prompt_capabilities": {
      "embedded_context": true,
      "image": true,
      "audio": true
    }
  },
  "auth_methods": []
}
```

**Loom 声明的能力**:

| 能力 | 值 | 说明 |
|------|-----|------|
| `load_session` | `true` | 支持加载历史 session |
| `session_capabilities.list` | `{}` | 支持列出所有 session |
| `session_capabilities.fork` | `{}` | 支持分叉 session |
| `prompt_capabilities.embedded_context` | `true` | 支持 Resource 类型内容 |
| `prompt_capabilities.image` | `true` | 支持 Image 类型内容 |
| `prompt_capabilities.audio` | `true` | 支持 Audio 类型内容 |
| `auth_methods` | `[]` | 不需要认证 |

**Loom 实现** (`agent.rs`):
- 返回 Loom 版本号和 agent 信息
- 保存客户端能力到 `client_capabilities`（用于后续 fs/terminal 工具判断）
- 保存 `ConnectionTo<Client>` 供后续 Agent→Client 调用使用

---

### authenticate

**何时**: 仅在 Agent 返回 `auth_required` 错误后调用。Loom 永远不会触发。

**JSON-RPC Method**: `authenticate`

**Request**:
```json
{
  "method": "authenticate",
  "auth": {}
}
```

**Response**:
```json
{}
```

**Loom 实现**: 直接返回成功（空响应）。Loom 不需要认证，`auth_methods` 为空数组。

---

### session/new

**何时**: 创建新对话 session。

**JSON-RPC Method**: `session/new`

**Request**:
```json
{
  "working_directory": "/path/to/project",
  "mcp_servers": [
    {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path"],
      "env": {}
    }
  ]
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `working_directory` | string (path) | ❌ | 工作目录，传递给 Loom `RunOptions::working_folder` |
| `mcp_servers` | array | ❌ | MCP 服务器配置列表 |

**Response**:
```json
{
  "session_id": "sess-abc123",
  "modes": [
    {
      "id": "default",
      "name": "Code Agent",
      "state": { "enabled": true }
    },
    {
      "id": "ask",
      "name": "Ask",
      "state": { "enabled": true }
    }
  ],
  "config_options": [
    {
      "id": "model",
      "name": "Model",
      "type": "string",
      "values": [
        { "name": "GPT-4o", "value": "openai/gpt-4o" },
        { "name": "Claude Sonnet", "value": "anthropic/claude-sonnet-4" }
      ]
    },
    {
      "id": "mode",
      "name": "Agent Mode",
      "type": "string"
    }
  ]
}
```

| 字段 | 说明 |
|------|------|
| `session_id` | Agent 生成的唯一 ID，与 Loom `thread_id` 1:1 对应 |
| `modes` | 可用 agent 模式列表（来自 `~/.loom/agents/` profiles） |
| `config_options` | 可配置选项列表（model、mode） |

**Loom 实现**:
1. 生成 UUID 作为 session_id
2. 创建 `SessionEntry`，建立 session_id ↔ thread_id 映射
3. 加载上次使用的模型（`last_model` 持久化）
4. 从 `AgentRegistry` 获取所有 agent profiles 转为 modes
5. 从 `ModelProvider` 获取所有可用模型转为 config_options
6. 更新日志路径（加入 session working_folder）

---

### session/prompt

**何时**: 用户在 IDE 中发送消息。核心方法。

**JSON-RPC Method**: `session/prompt`

**Request**:
```json
{
  "session_id": "sess-abc123",
  "content_blocks": [
    { "type": "text", "text": "帮我重构这个函数" },
    { "type": "image", "source": { "type": "base64", "data": "...", "media_type": "image/png" } }
  ]
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `session_id` | string | ✅ | 目标 session |
| `content_blocks` | ContentBlock[] | ✅ | 用户输入内容 |

**支持的 ContentBlock 类型**:

| 类型 | Loom 支持 | 说明 |
|------|-----------|------|
| `text` | ✅ | 纯文本/Markdown |
| `resource_link` | ✅ | 资源 URI 引用 |
| `image` | ✅ | Base64 编码图片 |
| `audio` | ✅ | Base64 编码音频 |
| `resource` | ✅ | 嵌入资源（需要 `embeddedContext` 能力） |

**Response**:
```json
{
  "stop_reason": "end_turn"
}
```

| stop_reason | 说明 |
|-------------|------|
| `end_turn` | 正常完成 |
| `cancelled` | 用户取消 |

**Loom 实现**:

1. **查找 session** → 获取 `SessionEntry`、working_directory、thread_id
2. **解析内容** → `content_blocks_to_message()` 将 ContentBlock 转为 `UserContent`
3. **检查内置命令**:
   - `/reset` → 清除 thread 上下文
   - `/goal <desc>` → 启动 GoalRunner
4. **解析模型** → tier-aware 模型解析（优先级：显式选择 > profile 配置 > tier > 默认）
5. **构建 `RunOptions`**:
   - agent name
   - extra_tools（ACP 客户端工具：fs_read、fs_write 等）
   - bash_executor（LocalCommandExecutor）
6. **执行** → `run_agent_with_options()`
7. **流式推送** → 通过 `SessionNotifier` 发送 `session/update` 通知
8. **返回** → `PromptResponse { stop_reason }`

**注意**: prompt 处理通过 `conn.spawn()` 在独立任务中执行，避免阻塞 JSON-RPC I/O 循环。

---

### session/fork

**何时**: 分叉当前 session 为新 session。

**JSON-RPC Method**: `session/fork`

**Request**:
```json
{
  "session_id": "sess-abc123",
  "working_directory": "/path/to/project"
}
```

**Response**:
```json
{
  "session_id": "sess-def456",
  "modes": [...],
  "config_options": [...]
}
```

**Loom 实现**:
- 复制源 session 的 config（model, mode）到新 session
- 生成新的 session_id
- *不复制对话历史*（历史通过 `session/load` 恢复）

---

### session/load

**何时**: 恢复历史 session。需要 `loadSession` 能力。

**JSON-RPC Method**: `session/load`

**Request**:
```json
{
  "session_id": "sess-abc123",
  "working_directory": "/path/to/project",
  "mcp_servers": [...]
}
```

**Response**:
```json
{
  "config_options": [...],
  "modes": [...]
}
```

**Loom 实现**:
1. 使用 session_id 作为 thread_id
2. 从 SQLite checkpoint 加载历史消息
3. 通过 `SessionNotifier::send_history()` 将历史消息转为 `session/update` 通知重放给 IDE
4. 重放顺序：User 消息 → Assistant 消息（含 tool_calls）→ Tool 结果
5. 返回当前 session 的 config 和 modes

**历史重放映射**:

| Loom Message | ACP SessionUpdate |
|--------------|-------------------|
| `Message::User` | `UserMessageChunk` |
| `Message::Assistant` | `AgentMessageChunk` + `AgentThoughtChunk`（reasoning）+ `ToolCall` |
| `Message::Tool` | `ToolCallUpdate`（Completed + content） |
| `Message::System` | 跳过（不发送给 IDE） |

---

### session/list

**何时**: 列出所有可恢复的 session。需要 `sessionCapabilities.list`。

**JSON-RPC Method**: `session/list`

**Request**:
```json
{
  "cwd": "/path/to/project",
  "cursor": null
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `cwd` | string | ❌ | 按工作目录过滤 |
| `cursor` | string | ❌ | 分页游标 |

**Response**:
```json
{
  "sessions": [
    {
      "session_id": "sess-abc123",
      "cwd": "/path/to/project",
      "title": "Refactor authentication module",
      "updated_at": "2026-05-01T10:30:00Z",
      "_meta": {
        "checkpoint_count": 15,
        "latest_step": 42
      }
    }
  ],
  "next_cursor": null
}
```

**Loom 实现**:
- 查询 SQLite checkpoints 表获取所有 session
- 分页暂未实现（返回所有 session）

---

### setSessionConfigOption

**何时**: 修改 session 配置。

**JSON-RPC Method**: `setSessionConfigOption`

**Request**:
```json
{
  "session_id": "sess-abc123",
  "config_option_id": "model",
  "value": "openai/gpt-4o"
}
```

**支持的 config_option_id**:

| config_id | 值格式 | 说明 |
|-----------|--------|------|
| `model` | `default` 或 `provider/model` | 切换 LLM 模型 |
| `mode` | mode_id string | 切换 Agent 模式 |

**Response**:
```json
{}
```

**Loom 实现**:
- 通过 `SessionConfigStore`（SQLite）持久化配置
- 配置在下次 `session/prompt` 时生效

---

### setSessionMode

**何时**: 切换 agent 模式。等效于 `setSessionConfigOption("mode", ...)`。

**JSON-RPC Method**: `setSessionMode`

**Request**:
```json
{
  "session_id": "sess-abc123",
  "mode_id": "ask"
}
```

**Response**:
```json
{}
```

**Loom 实现**:
- 验证 mode_id 是否存在于 `AgentRegistry`
- 更新 session config 的 current_agent
- 通过 `SessionNotifier::try_send_current_mode()` 推送 mode 变更通知

---

### setSessionModel

**何时**: 切换模型。等效于 `setSessionConfigOption("model", ...)`。

**JSON-RPC Method**: `setSessionModel`

**Request**:
```json
{
  "session_id": "sess-abc123",
  "model": "openai/gpt-4o"
}
```

**Response**:
```json
{}
```

**模型 ID 格式**: `provider/model`（如 `openai/gpt-4o`, `anthropic/claude-sonnet-4`）

---

## 通知方法

### session/cancel

**何时**: 用户取消当前生成。无响应。

**JSON-RPC Method**: `session/cancel`（Notification）

**Notification**:
```json
{
  "session_id": "sess-abc123"
}
```

**Loom 实现**:
1. 查找 session → 获取 `CancellationToken`
2. 调用 `cancel()` 设置取消标志
3. 如果有正在运行的 `run_agent_with_options`，会通过 `CancellationToken` 取消
4. prompt handler 检测取消后返回 `PromptResponse { stop_reason: Cancelled }`

---

## Agent → Client 方法

以下方法由 Loom Agent 发起，通过 `ConnectionTo<Client>` 调用 IDE。

### session/update (Notification)

**方向**: Agent → Client（通知，无响应）

**JSON-RPC Method**: `session/update`

**用途**: 推送 agent 的实时状态（文本输出、工具调用、计划等）。

**SessionUpdate 变体**:

| 变体 | 方向 | 说明 | Loom 来源 |
|------|------|------|-----------|
| `agent_message_chunk` | Agent→Client | Agent 文本输出块（流式） | LLM 文本生成 |
| `agent_thought_chunk` | Agent→Client | Agent 推理/思考块 | LLM Thinking 输出、TaskStart |
| `user_message_chunk` | Agent→Client | 用户消息块（仅历史重放） | `Message::User` 回放 |
| `tool_call` | Agent→Client | 新工具调用开始（Pending） | Act 节点发起工具调用 |
| `tool_call_update` | Agent→Client | 工具调用状态更新 | 工具开始执行/执行完成 |
| `plan` | Agent→Client | 执行计划 | `todo_write` 工具结果解析 |
| `current_mode_update` | Agent→Client | 当前模式变更 | `setSessionMode` 触发 |
| `session_info_update` | Agent→Client | Session 元数据更新 | title 节点输出 |

**ToolCall 状态流转**:
```
Pending → (request_permission?) → InProgress → Completed | Failed
```

**ToolCallUpdate content 类型**:

| 类型 | 说明 |
|------|------|
| `Text` | 纯文本结果 |
| `Diff` | 文件差异（path, old_text, new_text） |
| `Terminal` | 终端引用（terminal_id） |

**ToolKind 映射**（工具名 → 显示类别）:

| 工具名关键词 | ToolKind | 说明 |
|-------------|----------|------|
| read | `Read` | 读取操作 |
| write, edit | `Edit` | 编辑操作 |
| delete, remove | `Delete` | 删除操作 |
| move, rename | `Move` | 移动操作 |
| search, grep, glob | `Search` | 搜索操作 |
| run, bash, command, exec, shell | `Execute` | 执行操作 |
| think, reason | `Think` | 思考操作 |
| fetch | `Fetch` | 获取操作 |
| switch_mode, set_mode | `SwitchMode` | 模式切换 |

---

### session/request_permission

**方向**: Agent → Client → Agent（请求-响应）

**JSON-RPC Method**: `session/request_permission`

**用途**: Agent 请求用户授权执行工具。

**Request**:
```json
{
  "session_id": "sess-abc123",
  "tool_call_update": {
    "tool_call_id": "tool-uuid-123",
    "status": "pending",
    "title": "Editing src/main.rs"
  },
  "permission_options": [
    { "id": "allow_once", "name": "Allow Once" },
    { "id": "deny_once", "name": "Deny Once" }
  ]
}
```

**Response**:
```json
{
  "outcome": {
    "permission_option_id": "allow_once"
  }
}
```

或用户取消时：
```json
{
  "outcome": "cancelled"
}
```

**工具调用与权限的执行顺序**:
1. Agent 发送 `session/update` (ToolCall, status: Pending)
2. Agent 发送 `session/request_permission`
3. 如果允许: Agent 发送 `session/update` (ToolCallUpdate, InProgress → 执行 → Completed/Failed)
4. 如果拒绝或取消: Agent 发送 `session/update` (ToolCallUpdate, Failed)，返回 `StopReason::Cancelled`

---

### fs/read_text_file

**方向**: Agent → Client（请求）

**前提**: Client 在 `initialize` 中声明 `fs.read_text_file: true`

**Request**:
```json
{
  "session_id": "sess-abc123",
  "path": "src/main.rs",
  "line": 42,
  "limit": 100
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `session_id` | string | ✅ | Session ID |
| `path` | string | ✅ | 文件路径（相对于 cwd 或绝对路径） |
| `line` | u32 | ❌ | 起始行号（1-based） |
| `limit` | u32 | ❌ | 读取行数限制 |

**Response**:
```json
{
  "content": "fn main() {\n    println!(\"hello\");\n}"
}
```

**Loom 使用**: 当客户端支持时，Loom 的 `read` 工具优先通过此方法读取文件；否则回退到本地文件系统。

---

### fs/write_text_file

**方向**: Agent → Client（请求）

**前提**: Client 在 `initialize` 中声明 `fs.write_text_file: true`

**Request**:
```json
{
  "session_id": "sess-abc123",
  "path": "src/main.rs",
  "content": "fn main() {\n    println!(\"updated\");\n}"
}
```

**Response**:
```json
{}
```

**Loom 使用**: 当客户端支持时，Loom 的 `edit`/`write` 工具优先通过此方法写入文件。

---

### terminal/create

**方向**: Agent → Client（请求）

**前提**: Client 在 `initialize` 中声明 terminal 能力

**Request**:
```json
{
  "session_id": "sess-abc123",
  "command": "npm",
  "args": ["test"],
  "env": [{ "name": "NODE_ENV", "value": "test" }],
  "cwd": "/path/to/project",
  "output_byte_limit": 1048576
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `session_id` | string | ✅ | Session ID |
| `command` | string | ✅ | 要执行的命令 |
| `args` | string[] | ❌ | 命令参数 |
| `env` | EnvVariable[] | ❌ | 环境变量 |
| `cwd` | string | ❌ | 工作目录 |
| `output_byte_limit` | u64 | ❌ | 输出字节限制 |

**Response**:
```json
{
  "terminal_id": "term-abc123"
}
```

---

### terminal/output

**方向**: Agent → Client（请求）

**Request**:
```json
{
  "session_id": "sess-abc123",
  "terminal_id": "term-abc123"
}
```

**Response**:
```json
{
  "output": "test results...",
  "truncated": false,
  "exit_status": {
    "exit_code": 0,
    "signal": null
  }
}
```

---

### terminal/wait_for_exit

**方向**: Agent → Client（请求）

**Request**:
```json
{
  "session_id": "sess-abc123",
  "terminal_id": "term-abc123"
}
```

**Response**:
```json
{
  "exit_status": {
    "exit_code": 0,
    "signal": null
  }
}
```

---

### terminal/kill

**方向**: Agent → Client（请求）

**Request**:
```json
{
  "session_id": "sess-abc123",
  "terminal_id": "term-abc123"
}
```

**Response**:
```json
{}
```

---

### terminal/release

**方向**: Agent → Client（请求）

**用途**: 释放终端资源，通知客户端可以关闭终端。

**Request**:
```json
{
  "session_id": "sess-abc123",
  "terminal_id": "term-abc123"
}
```

**Response**:
```json
{}
```

---

## 数据类型参考

### ToolCallStatus

| 值 | 说明 |
|-----|------|
| `Pending` | 等待执行/权限确认 |
| `InProgress` | 正在执行 |
| `Completed` | 执行成功 |
| `Failed` | 执行失败 |

### StopReason

| 值 | 说明 |
|-----|------|
| `end_turn` | 正常完成 |
| `cancelled` | 用户取消 |

### ToolKind

| 值 | 说明 |
|-----|------|
| `Read` | 读取文件 |
| `Edit` | 编辑文件 |
| `Delete` | 删除文件 |
| `Move` | 移动/重命名 |
| `Search` | 搜索 |
| `Execute` | 执行命令 |
| `Think` | 思考/推理 |
| `Fetch` | 网络请求 |
| `SwitchMode` | 模式切换 |
| `Other` | 其他 |

### ToolCallContent

| 类型 | 字段 | 说明 |
|------|------|------|
| `Text` | text | 纯文本结果 |
| `Diff` | path, old_text, new_text | 文件差异 |
| `Terminal` | terminal_id | 终端引用 |

---

## 未实现的 ACP 方法

以下 ACP 协议方法在当前版本中 *未* 实现：

| 方法 | 状态 | 说明 |
|------|------|------|
| `session/close` | ❌ 未实现 | 0.12.x 已 stable，当前版本未支持 |
| `session/resume` | ❌ 未实现 | 0.12.x 已 stable |
| `session/delete` | ❌ 未实现 | 0.12.x unstable feature |
| `session/usage` | ❌ 未实现 | 0.12.x unstable feature |
| `logout` | ❌ 未实现 | 0.12.x unstable feature |
| `mcp_over_acp` | ❌ 未实现 | 0.12.x unstable feature |
| `session_additional_directories` | ❌ 未实现 | 0.12.x unstable feature |
| `session/request_permission` | ⚠️ 部分实现 | 框架已注册但 Loom 尚未在工具执行前调用 |

---

## 交互时序图

### 完整 Prompt Turn

```
IDE (Client)                          loom-acp (Agent)
    │                                       │
    │  initialize                           │
    │──────────────────────────────────────>│
    │<──────────────────────────────────────│  InitializeResponse
    │                                       │
    │  session/new                          │
    │──────────────────────────────────────>│
    │<──────────────────────────────────────│  NewSessionResponse + modes + config
    │                                       │
    │  session/prompt                       │
    │──────────────────────────────────────>│
    │                                       │  (spawn async task)
    │<──────────────────────────────────────│  session/update: agent_message_chunk
    │<──────────────────────────────────────│  session/update: agent_message_chunk
    │<──────────────────────────────────────│  session/update: tool_call (Pending)
    │<──────────────────────────────────────│  session/update: tool_call_update (InProgress)
    │<──────────────────────────────────────│  session/update: tool_call_update (Completed)
    │<──────────────────────────────────────│  session/update: agent_message_chunk
    │<──────────────────────────────────────│  PromptResponse { stop_reason: "end_turn" }
    │                                       │
```

### 带权限确认的 Prompt Turn

```
IDE (Client)                          loom-acp (Agent)
    │                                       │
    │  session/prompt                       │
    │──────────────────────────────────────>│
    │<──────────────────────────────────────│  session/update: tool_call (Pending)
    │                                       │
    │  session/request_permission           │
    │<──────────────────────────────────────│  (Agent 请求执行权限)
    │──────────────────────────────────────>│  { outcome: "allow_once" }
    │                                       │
    │<──────────────────────────────────────│  session/update: tool_call_update (InProgress)
    │<──────────────────────────────────────│  session/update: tool_call_update (Completed)
    │<──────────────────────────────────────│  PromptResponse { stop_reason: "end_turn" }
```

### 取消 Prompt

```
IDE (Client)                          loom-acp (Agent)
    │                                       │
    │  session/prompt                       │
    │──────────────────────────────────────>│
    │<──────────────────────────────────────│  session/update: agent_message_chunk ...
    │                                       │  (用户点击取消)
    │  session/cancel (notification)        │
    │──────────────────────────────────────>│
    │<──────────────────────────────────────│  PromptResponse { stop_reason: "cancelled" }
```

---

## 相关文档

- [ACP Implementation Guide](./ACP_IMPLEMENTATION.md) — 架构和模块实现细节
- [ACP Upgrade Guide](./ACP_UPGRADE_GUIDE.md) — 0.11.x → 0.12.x 升级指南
- [ACP Protocol Spec](https://agentclientprotocol.com) — 官方协议规范
- [ACP Rust SDK](https://github.com/agentclientprotocol/rust-sdk) — Rust SDK 源码
