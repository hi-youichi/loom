# Rust Agent Client Protocol (ACP) 官方协议索引文档

> **Agent Client Protocol (ACP)** 是一个标准化 AI Agent 与客户端（IDE、CLI 等）之间通信的协议。
> 基于 JSON-RPC 2.0 规范，支持工具调用、权限请求、流式响应等功能。

**最新协议版本**: v1 (稳定) | v2 (草案，实验性)
**官网**: https://agentclientprotocol.com
**Rust SDK**: https://github.com/agentclientprotocol/rust-sdk
**最后更新**: 2026-07-05

---

## 📚 目录

1. [官方资源](#官方资源)
2. [协议概览](#协议概览)
3. [Rust SDK 生态](#rust-sdk-生态)
4. [协议核心概念](#协议核心概念)
5. [协议方法清单](#协议方法清单)
6. [核心流程](#核心流程)
7. [能力与扩展](#能力与扩展)
8. [实现指南](#实现指南)
9. [opencode HTTP+SSE Loom Server 实现映射](#附录-hopencode-httpsse-loom-server-实现映射)

---

## 官方资源

### 官网与文档
- **主站**: https://agentclientprotocol.com
- **Rust 库页面**: https://agentclientprotocol.com/libraries/rust
- **文档索引**: https://agentclientprotocol.com/llms.txt

### 官方公告（按时间倒序）
- [ACP Registry 已稳定](https://agentclientprotocol.com/announcements/acp-agent-registry-stabilized.md) (2026-03-09)
- [实现信息](https://agentclientprotocol.com/announcements/implementation-information.md) (2025-10-24)
- [Logout 方法已稳定](https://agentclientprotocol.com/announcements/logout-method-stabilized.md) (2026-05-21)
- [Session Close 已稳定](https://agentclientprotocol.com/announcements/session-close-stabilized.md) (2026-04-23)
- [Session Config Options 已稳定](https://agentclientprotocol.com/announcements/session-config-options-stabilized.md) (2026-02-04)
- [Session List 已稳定](https://agentclientprotocol.com/announcements/session-list-stabilized.md) (2026-03-09)
- [Session Resume 已稳定](https://agentclientprotocol.com/announcements/session-resume-stabilized.md) (2026-04-22)

### RFDs (Request for Discussion)
ACP 使用 RFD 机制进行提案和讨论：

| RFD | 状态 | 描述 |
|-----|------|------|
| [v2 Overview](https://agentclientprotocol.com/rfds/v2/overview) | Draft | ACP v2 提案跟踪 |
| [v2 Message Updates](https://agentclientprotocol.com/rfds/v2/message-updates) | Draft | v2 消息更新和块 |
| [v2 Tool Call Updates](https://agentclientprotocol.com/rfds/v2/tool-call-updates) | Draft | v2 工具调用更新 |
| [v2 Prompt Lifecycle](https://agentclientprotocol.com/rfds/v2/prompt) | Draft | v2 Prompt 生命周期 |
| [ACP Agent Registry](https://agentclientprotocol.com/rfds/acp-agent-registry) | Completed | ACP Agent 注册表（已稳定） |
| [Session List](https://agentclientprotocol.com/rfds/session-list) | Completed | 会话列表（已稳定） |
| [Session Config Options](https://agentclientprotocol.com/rfds/session-config-options) | Completed | 会话配置选项（已稳定） |
| [Session Close](https://agentclientprotocol.com/rfds/session-close) | Completed | 会话关闭（已稳定） |
| [Session Resume](https://agentclientprotocol.com/rfds/session-resume) | Completed | 会话恢复（已稳定） |
| [Logout Method](https://agentclientprotocol.com/rfds/logout-method) | Completed | 退出方法（已稳定） |

### 协议规范文档
| 文档 | URL | 描述 |
|------|-----|------|
| Overview | [protocol/v1/overview](https://agentclientprotocol.com/protocol/v1/overview) | 协议工作原理 |
| Schema | [protocol/v1/schema](https://agentclientprotocol.com/protocol/v1/schema) | 完整 Schema 定义 |
| Initialization | [protocol/v1/initialization](https://agentclientprotocol.com/protocol/v1/initialization) | 初始化握手流程 |
| Authentication | [protocol/v1/authentication](https://agentclientprotocol.com/protocol/v1/authentication) | 认证流程 |
| Session Setup | [protocol/v1/session-setup](https://agentclientprotocol.com/protocol/v1/session-setup) | 会话建立 |
| Prompt Turn | [protocol/v1/prompt-turn](https://agentclientprotocol.com/protocol/v1/prompt-turn) | 提示词轮次 |
| Tool Calls | [protocol/v1/tool-calls](https://agentclientprotocol.com/protocol/v1/tool-calls) | 工具调用机制 |
| Session Modes | [protocol/v1/session-modes](https://agentclientprotocol.com/protocol/v1/session-modes) | 会话模式 |
| Session List | [protocol/v1/session-list](https://agentclientprotocol.com/protocol/v1/session-list) | 会话列表 |
| Session Delete | [protocol/v1/session-delete](https://agentclientprotocol.com/protocol/v1/session-delete) | 会话删除 |
| Extensibility | [protocol/v1/extensibility](https://agentclientprotocol.com/protocol/v1/extensibility) | 扩展机制 |

### 起步指南
- [Introduction](https://agentclientprotocol.com/get-started/introduction) - ACP 简介
- [Agents](https://agentclientprotocol.com/get-started/agents.md) - 实现 Agent
- [Clients](https://agentclientprotocol.com/get-started/clients.md) - 实现 Client
- [ACP Registry](https://agentclientprotocol.com/get-started/registry.md) - ACP 注册表

---

## 协议概览

### 核心特性
- **基于 JSON-RPC 2.0**: 标准请求-响应和通知机制
- **双向通信**: Agent 和客户端都可发起方法调用
- **流式更新**: 实时推送处理进度
- **权限管理**: 工具调用前的用户授权
- **多会话支持**: 同时管理多个对话会话
- **MCP 集成**: Model Context Protocol 支持
- **能力协商**: 运行时协商双方支持的功能

### 两种消息类型
1. **Methods (方法)**: 请求-响应对，期望返回结果或错误
2. **Notifications (通知)**: 单向消息，不期望响应

### 基本原则
- 所有 ACP 对象属性使用 `camelCase`
- 字段判别器的字符串值使用 `snake_case`
- JSON-RPC 信封字段遵循 JSON-RPC 2.0 规范

### 协议版本

#### v1 (当前稳定版本)
- 完整的会话管理、权限请求、流式更新
- 支持 MCP（Model Context Protocol）集成
- 客户端文件系统和终端执行接口
- 被所有主流 IDE 和 Agent 实现

#### v2 (草案，实验性)
**状态**: 需要 `unstable_protocol_v2` feature 启用
**当前状态**: 线格式与 v1 相同，正在演进中

**计划的主要变更**:
1. **移除客户端文件系统和终端接口**: 包括 `clientCapabilities.fs`、`clientCapabilities.terminal`、所有 `fs/*` 和 `terminal/*` 方法
2. **统一能力命名**: 使用单一的 `capabilities` 字段替代 `clientCapabilities` 和 `agentCapabilities`
3. **MCP 传输模型对齐**: 移除废弃的 HTTP+SSE 传输，`session.mcp.stdio` 作为可选能力
4. **改进的消息更新**: 支持全消息更新（upsert）和流式块，支持 `user_message`、`agent_message`、`agent_thought`
5. **改进的工具调用更新**: 单一的 `tool_call_update` 用于创建和更新，新增 `tool_call_content_chunk` 用于流式内容
6. **变更的 Prompt 生命周期**: Agent 在接受 prompt 时立即响应，而不是等待轮次结束

**启用 v2 (仅用于实验)**:
```toml
[dependencies.agent-client-protocol]
version = "0.12.1"
features = ["unstable_protocol_v2"]
```

---

## Rust SDK 生态

### 核心仓库
**GitHub**: https://github.com/agentclientprotocol/rust-sdk

### Crate 结构

```
rust-sdk/
├── agent-client-protocol/              # 核心协议 SDK
├── agent-client-protocol-tokio/        # Tokio 工具（进程生成）
├── agent-client-protocol-http/         # HTTP/SSE/WebSocket 传输
├── agent-client-protocol-rmcp/         # rmcp 集成
├── agent-client-protocol-cookbook/     # 使用模式（渲染为 rustdoc）
├── agent-client-protocol-derive/       # Proc 宏
├── agent-client-protocol-conductor/    # Conductor 二进制和库
├── agent-client-protocol-test/         # 测试工具和 fixtures
├── agent-client-protocol-trace-viewer/ # Trace 可视化工具
└── yopo/                               # "You Only Prompt Once" 示例客户端
```

### 核心 Crate

| Crate | 版本 | 描述 |
|-------|------|------|
| `agent-client-protocol` | **v0.12.1** | 核心 SDK，构建客户端、代理和代理服务器 |
| `agent-client-protocol-schema` | **v1.2.0** | 协议线格式类型（请求、响应、通知） |
| `agent-client-protocol-tokio` | v0.11.1 | Tokio 工具，生成和连接 Agent 进程 |
| `agent-client-protocol-rmcp` | v0.13.1 | rmcp 集成，提供 MCP 工具 |
| `agent-client-protocol-derive` | v1.0.1 | JSON-RPC trait 的派生宏 |
| `agent-client-protocol-conductor` | - | 代理链编排 |

### 依赖关系图
```
                    agent-client-protocol (核心 SDK)
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        │                           │                           │
agent-client-protocol-tokio  agent-client-protocol-rmcp  agent-client-protocol-http
        │                           │                           │
        └───────────────┬───────────┴───────────────────────────┘
                        │
            agent-client-protocol-conductor
                        │
            agent-client-protocol-cookbook
```

---

## 协议核心概念

### 角色（Roles）
ACP 定义了两种角色：
- **Client**: 客户端（IDE、CLI 等），发起提示词请求
- **Agent**: Agent 进程，处理提示词并返回响应

### 会话（Sessions）
- 每个 Agent 可以管理多个独立会话
- 会话通过 `session/new` 创建
- 可以通过 `session/load` 恢复
- 可以通过 `session/list` 发现
- 可以通过 `session/delete` 删除

### Prompt Turn（提示词轮次）
一个完整的交互流程：
```
Client ───session/prompt───▶ Agent
       ◀──session/update──── (流式更新，多次)
       ◀──session/prompt response
```

### 取消机制
- Client 发送 `session/cancel` 通知（无需响应）
- Agent 收到后应：
  - 尽快停止所有 LLM 请求
  - 中断所有进行中的工具调用
  - 发送待处理的 `session/update` 通知
  - 用 `StopReason::Cancelled` 响应原始 `session/prompt`

### 权限请求
- Agent 可以通过 `session/request_permission` 请求用户授权
- 返回可选项：`allow_once`、`allow_always`、`reject_once`、`reject_always`
- 支持自动批准策略（例如：`allow_all_for_terminal`）

---

## 协议方法清单

### 协议级方法（Protocol Level）

#### 1. initialize
- **方向**: Client → Agent
- **类型**: Request
- **描述**: 协商协议版本和能力
- **请求**: `InitializeRequest`
  ```json
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
      "protocolVersion": 1,
      "capabilities": {...},
      "clientInfo": {...}
    }
  }
  ```
- **响应**: `InitializeResponse`
  ```json
  {
    "jsonrpc": "2.0",
    "id": 1,
    "result": {
      "protocolVersion": 1,
      "capabilities": {...},
      "agentInfo": {...}
    }
  }
  ```

#### 2. authenticate
- **方向**: Client → Agent
- **类型**: Request
- **描述**: 使用指定认证方法认证
- **能力要求**: 无（Agent 在 `initialize` 响应中广告 `authMethods`）
- **请求**: `AuthenticateRequest`
- **响应**: `AuthenticateResponse | void`

#### 3. logout
- **方向**: Client → Agent
- **类型**: Request
- **描述**: 结束当前认证状态
- **能力要求**: `agentCapabilities.auth.logout`
- **请求**: `LogoutRequest`
- **响应**: `LogoutResponse`

#### 4. extMethod
- **方向**: 双向
- **类型**: Request
- **描述**: 自定义扩展请求
- **命名规则**: 方法名必须以 `_` 开头
- **请求**: 自定义
- **响应**: 自定义

#### 5. extNotification
- **方向**: 双向
- **类型**: Notification
- **描述**: 自定义扩展通知
- **命名规则**: 方法名必须以 `_` 开头

---

### 会话方法（Session Methods）

#### 6. session/new
- **方向**: Client → Agent
- **类型**: Request
- **描述**: 创建新会话
- **能力要求**: 无（所有 Agent 必须支持）
- **请求**: `NewSessionRequest`
- **响应**: `NewSessionResponse`
  ```json
  {
    "sessionId": "sess_abc123def456"
  }
  ```

#### 7. session/load
- **方向**: Client → Agent
- **类型**: Request
- **描述**: 加载现有会话并重放对话历史
- **能力要求**: `sessionCapabilities.loadSession`
- **请求**: `LoadSessionRequest`
- **响应**: `LoadSessionResponse`

#### 8. session/resume
- **方向**: Client → Agent
- **类型**: Request
- **描述**: 恢复现有会话（不重放历史）
- **能力要求**: `sessionCapabilities.resumeSession`
- **请求**: `ResumeSessionRequest`
- **响应**: `ResumeSessionResponse`

#### 9. session/prompt
- **方向**: Client → Agent
- **类型**: Request
- **描述**: 发送用户提示词
- **能力要求**: 无（所有 Agent 必须支持）
- **请求**: `PromptRequest`
  ```json
  {
    "sessionId": "sess_abc123",
    "prompt": {...},
    "options": {...}
  }
  ```
- **响应**: `PromptResponse`
  ```json
  {
    "stopReason": "end_turn", // 或 "cancelled", "error"
    "cost": {...},
    "annotations": [...]
  }
  ```

#### 10. session/cancel
- **方向**: Client → Agent
- **类型**: Notification
- **描述**: 取消正在进行操作
- **能力要求**: 无（所有 Agent 必须支持）
- **请求**: `CancelNotification`

#### 11. session/close
- **方向**: Client → Agent
- **类型**: Request
- **描述**: 关闭活动会话
- **能力要求**: `sessionCapabilities.close`
- **请求**: `CloseSessionRequest`
- **响应**: `CloseSessionResponse`

#### 12. session/list
- **方向**: Client → Agent
- **类型**: Request
- **描述**: 列出 Agent 已知的会话
- **能力要求**: `sessionCapabilities.list`
- **请求**: `ListSessionsRequest`
  ```json
  {
    "cwd": "/home/user/project",  // 可选：过滤工作目录
    "cursor": "eyJwYWdlIjogMn0="  // 可选：分页游标
  }
  ```
- **响应**: `ListSessionsResponse`
  ```json
  {
    "sessions": [...],
    "nextCursor": "..."  // 更多结果时提供
  }
  ```

#### 13. session/delete
- **方向**: Client → Agent
- **类型**: Request
- **描述**: 从会话列表中删除会话
- **能力要求**: `sessionCapabilities.delete`
- **请求**: `DeleteSessionRequest`
- **响应**: `DeleteSessionResponse`

#### 14. session/set_mode
- **方向**: Client → Agent
- **类型**: Request
- **描述**: 切换 Agent 操作模式
- **能力要求**: 无（Agent 在 `session/new` 响应中广告可用模式）
- **请求**: `SetSessionModeRequest`
- **响应**: `SetSessionModeResponse | void`

#### 15. session/set_config_option
- **方向**: Client → Agent
- **类型**: Request
- **描述**: 设置会话配置选项
- **能力要求**: 无（Agent 在 `session/new` 响应中广告可用选项）
- **请求**: `SetSessionConfigOptionRequest`
- **响应**: `SetSessionConfigOptionResponse`

---

### 客户端方法（Client Methods）

#### 16. session/request_permission
- **方向**: Agent → Client
- **类型**: Request
- **描述**: 请求工具调用权限
- **请求**: `RequestPermissionRequest`
  ```json
  {
    "sessionId": "sess_abc123",
    "toolCall": {...},
    "options": [
      {
        "optionId": "allow-once",
        "name": "Allow once",
        "kind": "allow_once"
      }
    ]
  }
  ```
- **响应**: `RequestPermissionResponse`
  ```json
  {
    "optionId": "allow-once"
  }
  ```

#### 17. fs/read_text_file
- **方向**: Agent → Client
- **类型**: Request
- **描述**: 读取文件内容
- **能力要求**: `clientCapabilities.fs.readTextFile`
- **请求**: `ReadTextFileRequest`
- **响应**: `ReadTextFileResponse`

#### 18. fs/write_text_file
- **方向**: Agent → Client
- **类型**: Request
- **描述**: 写入文件内容
- **能力要求**: `clientCapabilities.fs.writeTextFile`
- **请求**: `WriteTextFileRequest`
- **响应**: `WriteTextFileResponse`

#### 19. terminal/create
- **方向**: Agent → Client
- **类型**: Request
- **描述**: 创建新终端
- **能力要求**: `clientCapabilities.terminal`
- **请求**: `CreateTerminalRequest`
- **响应**: `CreateTerminalResponse`

#### 20. terminal/output
- **方向**: Agent → Client
- **类型**: Request
- **描述**: 获取终端输出
- **能力要求**: `clientCapabilities.terminal`
- **请求**: `TerminalOutputRequest`
- **响应**: `TerminalOutputResponse`

#### 21. terminal/wait_for_exit
- **方向**: Agent → Client
- **类型**: Request
- **描述**: 等待终端命令退出
- **能力要求**: `clientCapabilities.terminal`
- **请求**: `WaitForTerminalExitRequest`
- **响应**: `WaitForTerminalExitResponse`

#### 22. terminal/release
- **方向**: Agent → Client
- **类型**: Request
- **描述**: 释放终端
- **能力要求**: `clientCapabilities.terminal`
- **请求**: `ReleaseTerminalRequest`
- **响应**: `ReleaseTerminalResponse`

#### 23. terminal/kill
- **方向**: Agent → Client
- **类型**: Request
- **描述**: 杀死终端进程（不释放）
- **能力要求**: `clientCapabilities.terminal`
- **请求**: `KillTerminalRequest`
- **响应**: `KillTerminalResponse`

---

### 会话通知（Session Notifications）

#### 24. session/update
- **方向**: Agent → Client
- **类型**: Notification
- **描述**: 发送会话更新（流式）
- **更新类型**:
  - `agent_message_chunk` - Agent 响应片段
  - `user_message_chunk` - 用户消息片段
  - `thought_chunk` - 思考片段
  - `tool_call` - 工具调用
  - `tool_call_update` - 工具调用更新
  - `plan` - 计划
  - `available_commands_update` - 可用命令更新
  - `current_mode_update` - 当前模式更新
  - `config_option_update` - 配置选项更新
  - `session_info_update` - 会话信息更新
- **请求**: `SessionUpdateNotification`
  ```json
  {
    "jsonrpc": "2.0",
    "method": "session/update",
    "params": {
      "sessionId": "sess_abc123",
      "update": {
        "agentMessageChunk": {...}
      }
    }
  }
  ```

---

### 不稳定方法（Unstable Methods）

以下方法标记为 `unstable_`，可能在 future 版本中变更：

| 方法 | 描述 |
|------|------|
| `unstable_fork_session` | Fork 现有会话 |
| `unstable_list_sessions` | 列出会话（已稳定为 `session/list`） |
| `unstable_resume_session` | 恢复会话（已稳定为 `session/resume`） |
| `unstable_set_session_model` | 设置会话模型 |
| `unstable_list_providers` | 列出提供者 |
| `unstable_set_provider` | 设置提供者 |
| `unstable_disable_provider` | 禁用提供者 |
| `unstable_start_nes` | 启动 NES 会话 |
| `unstable_suggest_nes` | 建议 NES |
| `unstable_close_nes` | 关闭 NES 会话 |
| `unstable_did_open_document` | 文档打开通知 |
| `unstable_did_change_document` | 文档变更通知 |
| `unstable_did_close_document` | 文档关闭通知 |
| `unstable_did_save_document` | 文档保存通知 |
| `unstable_did_focus_document` | 文档聚焦通知 |
| `unstable_accept_nes` | 接受 NES 建议 |
| `unstable_reject_nes` | 拒绝 NES 建议 |
| `unstable_create_elicitation` | 创建 elicitation 表单 |
| `unstable_complete_elicitation` | 完成 elicitation |
| `unstable_connect_mcp` | 连接 MCP 服务器 |
| `unstable_cancel_request` | 取消请求 |

---

## 核心流程

### 1. 初始化流程
```
Client                     Agent
  │                          │
  │──── initialize ────────▶│
  │◀── initialize response ─│
  │  (protocolVersion,       │
  │   capabilities,          │
  │   authMethods)           │
  │                          │
  [可选: 认证]               │
  │──── authenticate ──────▶│
  │◀── authenticate response│
  │                          │
```

### 2. 会话创建流程
```
Client                     Agent
  │                          │
  │──── session/new ───────▶│
  │◀── session/new response │
  │  (sessionId,             │
  │   modes,                 │
  │   configOptions)         │
  │                          │
```

### 3. Prompt Turn 流程
```
Client                     Agent
  │                          │
  │──── session/prompt ────▶│
  │                          │
  │◀── session/update (plan) │
  │◀── session/update (tool_call) │
  │                          │
  │ [可选: 权限请求]         │
  │◀── session/request_permission │
  │──── permission response ─▶│
  │                          │
  │◀── session/update (tool_call_update) │
  │◀── session/update (agent_message_chunk) │
  │◀── session/update (agent_message_chunk) │
  │                          │
  │◀── session/prompt response│
  │  (stopReason, cost, etc.) │
```

### 4. 取消流程
```
Client                     Agent
  │                          │
  │──── session/prompt ────▶│
  │                          │
  │◀── session/update (chunk)│
  │                          │
  │──── session/cancel ────▶│ (notification)
  │                          │
  │◀── session/update (final)│
  │◀── session/prompt response│
  │  (stopReason: cancelled) │
```

### 5. 会话加载流程
```
Client                     Agent
  │                          │
  │──── session/load ──────▶│
  │                          │
  │◀── session/update (user_msg_1) │
  │◀── session/update (agent_msg_1)│
  │◀── session/update (tool_call_1)│
  │◀── session/update (tool_call_update_1)│
  │◀── session/update (user_msg_2) │
  │                          │
  │◀── session/load response│
  │                          │
  [现在可以继续发送 prompt] │
```

---

## 能力与扩展

### 客户端能力（Client Capabilities）

#### 文件系统
```json
{
  "fs": {
    "readTextFile": {},  // 支持 fs/read_text_file
    "writeTextFile": {}  // 支持 fs/write_text_file
  }
}
```

#### 终端
```json
{
  "terminal": {}  // 支持所有 terminal/* 方法
}
```

#### NES（不稳定）
```json
{
  "nes": {
    "start": true,
    "suggest": true,
    "close": true
  }
}
```

### Agent 能力（Agent Capabilities）

#### 认证
```json
{
  "auth": {
    "logout": {}  // 支持 logout 方法
  }
}
```

#### 会话
```json
{
  "session": {
    "loadSession": {},     // 支持 session/load
    "delete": {},          // 支持 session/delete
    "list": {},            // 支持 session/list
    "close": {},           // 支持 session/close
    "resumeSession": {},   // 支持 session/resume
    "additionalDirectories": {}  // 支持额外工作目录
  }
}
```

#### Prompt 能力
```json
{
  "prompt": {
    "supportedContentBlocks": ["text", "resource_link"]  // 必须
  }
}
```

#### MCP 能力
```json
{
  "mcp": {
    "http": true,      // 支持 MCP over HTTP
    "sse": true,       // 支持 MCP over SSE
    "acpTransport": true  // 支持 MCP-over-ACP
  }
}
```

### 扩展机制

#### 自定义方法
- 请求/通知方法名以 `_` 开头
- 使用 `_meta` 字段广告扩展能力

```json
{
  "capabilities": {
    "_meta": {
      "my_extension": {
        "version": "1.0.0",
        "methods": ["_my_custom_method"]
      }
    }
  }
}
```

#### 自定义能力
```json
{
  "clientCapabilities": {
    "_meta": {
      "customFeature": {
        "description": "A custom feature"
      }
    }
  }
}
```

---

## 实现指南

### Rust 快速开始

#### 1. 添加依赖
```toml
[dependencies]
agent-client-protocol = "1.0"
```

#### 2. 实现客户端
```rust
use agent_client_protocol::{Client, ConnectTo};
use agent_client_protocol::schema::{ProtocolVersion, v1::InitializeRequest};

Client.builder()
    .name("my-client")
    .connect_with(transport, async |cx| {
        // 初始化连接
        cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
            .block_task()
            .await?;
        
        Ok(())
    })
    .await?;
```

#### 3. 实现服务端
```rust
use agent_client_protocol::{Agent, ConnectTo};

Agent.builder()
    .name("my-agent")
    .connect_with(transport, async |cx| {
        // 处理 initialize 请求
        // 处理其他方法...
        Ok(())
    })
    .await?;
```

### Builder API

#### 注册处理器
```rust
Client.builder()
    .name("my-client")
    .on_receive_request::<ReadTextFileRequest, _>(|cx, request| async move {
        // 处理读取文件请求
        Ok(ReadTextFileResponse::new("file contents"))
    })
    .on_receive_notification::<CancelNotification>(|cx, notification| async move {
        // 处理取消通知
    })
    .connect_with(transport, async |cx| {
        // 连接逻辑
        Ok(())
    })
    .await?;
```

### 角色（Role）类型系统
- `ClientSide`: 客户端角色
- `AgentSide`: Agent 角色
- `Role`: 统一的角色 trait

### 组件（Component）抽象
- `ByteStreams`: 字节流传输（stdin/stdout, sockets, pipes）
- `Lines`: 行流传输

### 会话构建器
```rust
let session = cx.session_builder()
    .id("sess_abc123")
    .workspace("/home/user/project")
    .add_mcp_server(...)
    .build()
    .await?;
```

### Cookbook 模式

可用的 cookbook 模式（在 `agent-client-protocol-cookbook` crate 中）：
- 连接作为客户端
- 全局 MCP 服务器
- 带工作区上下文的每会话 MCP 服务器
- 构建 Agent 和可复用组件
- 使用 Conductor 运行代理链

### 示例代码

#### Agent 示例
- [agent.rs](https://github.com/agentclientprotocol/rust-sdk/blob/main/src/agent-client-protocol/examples/agent.rs)

#### Client 示例
- [client.rs](https://github.com/agentclientprotocol/rust-sdk/blob/main/src/agent-client-protocol/examples/client.rs)

### 相关工具

| 工具 | 描述 |
|------|------|
| `agent-client-protocol-conductor` | 运行代理链的二进制程序 |
| `agent-client-protocol-trace-viewer` | 交互式 trace 可视化工具 |

---

## 附录

### A. 错误处理
所有方法遵循 JSON-RPC 2.0 错误处理：
- 成功响应包含 `result` 字段
- 错误包含 `error` 对象，带有 `code` 和 `message`
- 通知永不接收响应（成功或失败）

### B. 分页
`session/list` 使用基于游标的分页：
- 请求包含可选的 `cursor`
- 响应在更多结果可用时提供 `nextCursor`

### C. 模式
Agent 可以提供多个操作模式：
- 模式影响系统提示词、工具可用性、权限策略
- Agent 可以通过 `current_mode_update` 通知模式变更

### D. 成本追踪
`PromptResponse` 可以包含成本信息：
```json
{
  "cost": {
    "currency": "USD",
    "amount": 0.0025
  }
}
```

### E. 注解（Annotations）
- 可选的元数据，客户端可用于控制对象的使用或显示
- 可在响应中返回

### F. 内容块（Content Blocks）
支持的提示词内容块类型：
- `text`: 纯文本（必须）
- `resource_link`: 资源链接（必须）
- `image`: 图像内容
- `audio`: 音频内容

### G. 工具调用类型
- `read`: 读取文件或数据
- `edit`: 修改文件或内容
- `delete`: 删除文件或数据
- `move`: 移动或重命名
- `search`: 搜索信息
- `execute`: 运行命令或代码
- `think`: 内部推理或规划
- `fetch`: 检索外部数据
- `other`: 其他工具类型（默认）

---

## 参考资料

### 官方链接
- [ACP Spec](https://agentclientprotocol.com)
- [Rust SDK GitHub](https://github.com/agentclientprotocol/rust-sdk)
- [Rust Crate](https://crates.io/crates/agent-client-protocol)
- [Rust Docs](https://docs.rs/agent-client-protocol)

### 其他语言 SDK
| 语言 | SDK |
|------|-----|
| TypeScript | [@agentclientprotocol/sdk](https://www.npmjs.com/package/@agentclientprotocol/sdk) |
| Python | [python-sdk](https://github.com/agentclientprotocol/python-sdk) |
| Kotlin | [acp-kotlin](https://github.com/agentclientprotocol/kotlin-sdk) |
| Java | [java-sdk](https://github.com/agentclientprotocol/java-sdk) |

### 社区与贡献
- [Contributor Communication](https://agentclientprotocol.com/community/communication.md)
- [GitHub Issues](https://github.com/agentclientprotocol/agent-client-protocol/issues)

---

**文档版本**: 1.1
**最后更新**: 2026-07-05
**协议版本**: v1 (稳定) | v2 (草案，实验性)

---

## 附录 H：opencode HTTP+SSE Loom Server 实现映射

> Last verified: 2026-07-13
> Server implementation: `apps/server` (`loom-server`)
> Route registry: `apps/server/src/routes.rs`

This index records the HTTP/SSE contract implemented by the Rust Loom agent
server for opencode External mode. The generated opencode SDK remains the
upstream source of truth; this file maps that contract to Rust artifacts and
verification gates.

## Sources of truth

1. opencode generated SDK:
   `packages/sdk/js/src/v2/gen/sdk.gen.ts` and `types.gen.ts`.
2. Rollout plan: `docs/design/loom-server-v2-protocol-rollout.md`.
3. Runtime route registry: `apps/server/src/routes.rs`.
4. Protocol tests: `apps/server/tests/protocol.rs`.
5. Executable smoke gates: `scripts/check-protocol.ps1` and
   `scripts/check-protocol.sh`.

The rollout plan was authored against an earlier v2 SDK snapshot. The server
therefore keeps those rollout URLs and also registers the current generated
resource-oriented aliases under `/api/app/*`, `/api/project/*`,
`/api/experimental/app/*`, `/api/config/agents*`, and related paths. Compatibility
aliases live in `handlers/v2_compat.rs` and are intentionally conservative
stubs outside the session/agent critical path.

## Critical bootstrap routes

| Client generation | Routes | Rust owner | Response rule |
|---|---|---|---|
| v1/current TUI | `/config`, `/config/providers`, `/provider`, `/agent`, `/path`, `/project`, `/project/current`, `/command`, `/mcp`, `/lsp`, `/formatter`, `/session/status`, `/experimental/capabilities` | `handlers/bootstrap.rs`, `handlers/mcp_pty_file.rs`, `handlers/lsp_formatter.rs`, `handlers/messages.rs`, `handlers/experimental.rs` | Bare SDK data; no `{data: ...}` wrapper |
| rollout v2 | `/api/health`, `/api/location`, `/api/path`, `/api/config`, `/api/provider`, `/api/agent`, `/api/model`, `/api/command`, `/api/skill`, `/api/reference`, `/api/integration` | `handlers/health.rs`, `handlers/bootstrap.rs`, `handlers/vcs_extra.rs` | Earlier-v2 compatibility shapes |
| current v2 aliases | `/api/app/agent`, `/api/app/model`, `/api/app/provider`, `/api/project`, `/api/project/current`, `/api/workspace` | `handlers/bootstrap.rs`, `handlers/v2_compat.rs` | Current generated SDK route/method aliases |

`/agent` is intentionally the v1 bootstrap URL used by the unchanged TUI; it
returns the discoverable `build` agent.

## Session and agent critical path

| Capability | Routes | Rust owner |
|---|---|---|
| Session CRUD | `GET/POST /session`, `GET/PATCH/DELETE /session/:id`, `/api/session*` aliases | `handlers/session.rs` |
| Current TUI prompt | `POST /session/:id/message` | `handlers/session.rs::prompt` |
| Rollout prompt | `POST /session/:id/prompt`, `POST /session/:id/prompt_async`, `POST /api/session/:id/agent` | `handlers/session.rs` |
| Agent execution | Loom `ReactBuildConfig` + `run_agent_from_config` | `agent_runner.rs` |
| Cancellation | `POST /session/:id/abort`, `POST /api/session/:id/interrupt` | `handlers/session.rs`, `state.rs` |
| Message/Part CRUD | `/session/:id/message*`, `/api/session/:id/message*` | `handlers/messages.rs` |
| Session projections | children, share, fork, init, summarize, todo, diff | `handlers/session.rs`, `handlers/messages.rs` |
| TUI control | `/tui/*`, `/control/next` | `handlers/control.rs`, `handlers/v2_compat.rs` |

The prompt handler persists user and assistant messages, emits busy/idle status,
runs the production Loom ReAct path, translates stream events, and uses
generation-safe `RunCancellation` cleanup so an older cancelled task cannot
remove a replacement run.

## SSE contract

| Channel | Wire envelope | Rust owner |
|---|---|---|
| `/event`, `/global/event` | `{directory, payload:{id,type,properties}}` | `sse.rs::event_stream` |
| `/api/event` | `{payload:{id,type,properties}}` | `sse.rs::api_event_stream` |

Every connection receives `server.connected`, business events, business-level
`server.heartbeat`, and transport keep-alive comments. Replay/cursor state is
held in `AppState.event_buffer`; `GET /api/session/:id/event?after=<id>` filters
by session and returns `{data,cursor,hasMore}`.

## Non-critical/stub route groups

| Group | Rust owner |
|---|---|
| Permission and question | `handlers/permission.rs`, `handlers/question.rs` |
| Revert lifecycle | `handlers/revert.rs` |
| Instance, MCP, PTY, file, find | `handlers/instance.rs`, `handlers/mcp_pty_file.rs` |
| Experimental resources/apps | `handlers/experimental.rs`, `handlers/v2_compat.rs` |
| Provider OAuth/auth | `handlers/provider_auth.rs`, `handlers/v2_compat.rs` |
| Global event/config/session control | `handlers/global_bus.rs` |

These endpoints are registered with the SDK method and return a stable JSON
stub where the feature is deliberately not implemented. They must not 404 or
405 for a valid generated-SDK call.

## Verification

Run all gates from the repository root:

```powershell
cargo fmt --all -- --check
cargo check -p loom-server
cargo test -p loom-server
cargo clippy -p loom-server --all-targets -- -D warnings
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/check-protocol.ps1
```

Linux/macOS:

```sh
bash scripts/check-protocol.sh
```

The protocol scripts boot `loom-server serve`, send an Authorization header,
validate both SSE envelopes, exercise stateful session CRUD, and probe P2/current
v2 compatibility routes. They intentionally avoid a paid LLM call; real prompt
execution requires the normal Loom provider/model environment.
