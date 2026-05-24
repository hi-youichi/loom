# Loom ACP Implementation Guide

> 本文档描述 `loom-acp` crate 的架构、协议映射、模块职责和内部实现细节。

## 概述

`loom-acp` 是一个独立的 Rust crate，实现了 [Agent Client Protocol (ACP)](https://agentclientprotocol.com) 的 **Agent 端**。IDE（Zed、JetBrains、Neovim 等）将其作为子进程启动，通过 **stdio** 使用 JSON-RPC 2.0 通信，将 ACP 请求映射到 Loom 的 `run_agent_with_options` 执行引擎。

- **传输层**: 仅 stdio（newline-delimited JSON-RPC 2.0）
- **依赖版本**: `agent-client-protocol = "0.11.1"`（当前），最新为 0.12.1
- **协议版本**: ACP protocol version 1

## 架构

```
┌─────────────────────────────────────────────────────────┐
│  IDE (Zed / JetBrains / Neovim)          [Client]      │
└─────────────────────────────────────────────────────────┘
      │ stdin (JSON-RPC Request)       ▲ stdout (Response/Notification)
      ▼                                │
┌─────────────────────────────────────────────────────────┐
│  loom-acp process                                       │
│  ┌────────────────────────────────────────────────────┐ │
│  │  Transport   run_stdio_loop() / AgentSideConnection│ │
│  └──────────────────────┬─────────────────────────────┘ │
│                         │                                │
│  ┌──────────────────────▼─────────────────────────────┐ │
│  │  Agent   LoomAcpAgent                              │ │
│  │  initialize / new_session / prompt / cancel / ...  │ │
│  └──┬───────────┬──────────────┬──────────────────────┘ │
│     │           │              │                         │
│  SessionStore  Content    StreamBridge                   │
│  SessionEntry  Parser     AnyStreamEvent → SessionUpdate │
│     │           │              │                         │
│     └───────────┴──────────────┘                         │
│                 │                                        │
│  ┌──────────────▼──────────────────────────────────────┐│
│  │  Loom Core  run_agent_with_options / build_config   ││
│  └─────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────┘
```

## 模块职责

### `main.rs` — CLI 入口

二进制入口点，处理启动、PID 文件、信号和生命周期。

| 功能 | 说明 |
|------|------|
| `main()` | 解析 CLI 参数，分发到 `run_server()` 或 `run_reload()` |
| `run_server()` | 加载配置、设置 panic hook、初始化 tokio runtime、运行 `run_stdio_loop()` |
| `run_reload()` | 子命令 `reload`：发送 SIGHUP 给运行中的进程（仅 Unix） |
| PID 管理 | 写入 `~/.loom/acp/loom-acp.pid`，进程退出时自动清理 |

**CLI 参数**:

```
loom-acp [OPTIONS] [COMMAND]

Options:
  --show-log-dir      打印日志目录路径并退出
  --log-level <LEVEL>  日志级别 (trace/debug/info/warn/error)，默认 info
  --log-file <PATH>    日志文件路径，默认 ~/.loom/acp/loom-acp.log
  --log-rotate <STR>   日志轮转策略 (none/daily/hourly/minutely)，默认 daily
  --log-format <FMT>   日志格式 (text/json)，默认 text

Commands:
  reload              发送 SIGHUP 触发热重载（仅 Unix）
```

**退出码**:
- `0`: 正常退出
- `203`: 收到 SIGHUP，需要重启（reload 模式）

### `lib.rs` — 核心循环

`run_stdio_loop()` 是核心入口，负责：

1. 初始化日志
2. 创建 `LoomAcpAgent` 和 session notification channel
3. 使用 `Agent.builder()` 注册所有 ACP 请求处理器
4. 通过 `connect_to(ByteStreams)` 启动 JSON-RPC I/O 循环
5. 连接关闭时返回 `StdioLoopResult`

**注册的处理器**:

| ACP 方法 | 处理函数 | 说明 |
|----------|----------|------|
| `initialize` | `agent.initialize()` | 能力协商 |
| `authenticate` | `agent.authenticate()` | 直接返回成功（Loom 不需要认证） |
| `session/new` | `agent.new_session()` | 创建新 session |
| `session/prompt` | `agent.prompt()` | 执行用户提示（spawn 异步任务避免阻塞 IO 循环） |
| `session/fork` | `agent.fork_session()` | 分叉 session |
| `session/load` | `agent.load_session()` | 加载已有 session 历史 |
| `session/list` | `agent.list_sessions()` | 列出所有 sessions |
| `setSessionConfigOption` | `agent.set_session_config_option()` | 设置 session 配置 |
| `setSessionMode` | `agent.set_session_mode()` | 切换 session 模式 |
| `setSessionModel` | `agent.set_session_model()` | 切换模型 |
| `session/cancel` | `agent.cancel()` | 取消当前生成 |

### `agent.rs` — LoomAcpAgent

核心 Agent 实现，映射 ACP 协议请求到 Loom 执行。

**`LoomAcpAgent` 结构体**:

```rust
pub struct LoomAcpAgent {
    sessions: SessionStore,           // session 状态管理
    agent_registry: AgentRegistry,    // agent profile → ACP mode 映射
    config_store: SessionConfigStore, // session 配置持久化 (SQLite)
    session_update_tx: Option<...>,   // session/update 通知发送通道
    terminal_mgr: Arc<TerminalManager>, // terminal 会话管理
    client_capabilities: RwLock<ClientCapabilitiesInfo>, // 客户端能力
    model_provider: Arc<dyn ModelProvider>, // 模型列表获取
}
```

**模型解析优先级**（`resolve_model_with_tier_awareness`）:

1. ACP 显式选择的 model（最高优先级）
2. Agent profile 配置的 model name
3. Agent profile 的 tier（通过 `resolve_tier_and_build_config`）
4. 配置文件的 default_provider + model
5. `config::default_model()` 兜底

**Session 配置选项**:

| config_id | 说明 |
|-----------|------|
| `model` | LLM 模型选择，支持 `default` 或 `provider/model` 格式 |
| `mode` | Agent 模式切换（对应 loom agent profiles） |

**Prompt 处理流程**:

1. 查找 session → 解析 content_blocks 为 UserContent
2. 检查内置命令（`/reset`, `/goal`）
3. 解析模型配置（tier-aware）
4. 构建 `RunOptions`，包括 agent、extra_tools、bash_executor
5. 调用 `run_agent_with_options()` 执行
6. 通过 StreamBridge 发送 `session/update` 通知
7. 返回 `PromptResponse`（StopReason::EndTurn 或 Cancelled）

### `session.rs` — SessionStore

管理 ACP session 状态，session_id 与 thread_id 1:1 映射。

**关键类型**:

- `SessionId`: session 标识符（newtype wrapper）
- `SessionEntry`: 单个 session 状态（working_directory, thread_id, session_config, cancel token）
- `SessionConfig`: session 配置（current_agent, model）
- `SessionStore`: 全局 session 表 + cancel 管理

**Session 生命周期**:

```
new_session() → create SessionEntry → session_id ↔ thread_id 映射
     │
prompt() → begin_prompt() (获取 CancellationToken) → run_agent → finish_prompt()
     │
cancel() → cancel_current_generation() (设置 cancel flag)
     │
fork_session() → 复制 config 到新 session
     │
load_session() → 从 SQLite checkpoint 恢复历史
```

### `content.rs` — ContentBlock 解析

将 ACP `ContentBlock` 列表转换为 Loom `UserContent`。

| ACP ContentBlock | Loom 支持 | 说明 |
|------------------|-----------|------|
| Text | ✅ | 纯文本/Markdown，拼接为字符串 |
| ResourceLink | ✅ | 资源 URI |
| Image | ✅ | Base64 图片 → `ContentPart::ImageBase64` |
| Audio | ✅ | Base64 音频 → `ContentPart::AudioBase64` |
| Resource | ✅ | 嵌入资源（需要 embeddedContext） |

输出类型：`UserContent::Text(String)` 或 `UserContent::Multimodal(Vec<ContentPart>)`

### `stream_bridge.rs` — 事件桥接

将 Loom 的 `AnyStreamEvent` 转换为 ACP `SessionUpdate` 通知。

**核心函数**:
- `loom_event_to_updates()`: 单个 Loom event → 零或多个 StreamUpdate
- `event_stream_to_session_updates()`: 处理事件流

**映射关系**:

| Loom Event | ACP SessionUpdate |
|------------|-------------------|
| Think/Text output | `agent_message_chunk` / `agent_thought_chunk` |
| Tool call start | `tool_call` (Pending) |
| Tool execution | `tool_call_update` (Running → Success/Failure) |
| Mode change | `current_mode_update` |

**`SessionNotifier`**: 封装 `mpsc::Sender<SessionNotification>`，提供 `try_send_event()` 和 `send_history()` 方法。

### `client_capabilities.rs` — 客户端能力检测

从 `initialize` 请求中检测客户端支持的能力。

| 能力 | 方法 | 说明 |
|------|------|------|
| `fs/read_text_file` | `can_read_text_file()` | 客户端支持读取文件 |
| `fs/write_text_file` | `can_write_text_file()` | 客户端支持写入文件 |
| Terminal | `supports_terminal()` | 客户端支持终端操作 |

### `tools/` — ACP 工具实现

提供通过 ACP 协议调用客户端能力的工具。

| 模块 | 说明 |
|------|------|
| `mod.rs` | 工具注册入口，`create_acp_tools()` 根据客户端能力创建工具列表 |
| `client_bridge.rs` | 客户端桥接 trait，定义与 IDE 通信的接口 |
| `fs_tools.rs` | 文件系统工具（fs/read, fs/write），通过 ACP 调用 IDE |
| `terminal_executor.rs` | Terminal 命令执行器 |

工具仅在客户端声明支持时可用，否则回退到本地执行。

### `agent_registry.rs` — Agent 注册表

将 Loom agent profiles 映射到 ACP session modes。

- `list_modes()`: 列出所有可用 agent profiles
- `mode_exists()`: 检查 mode 是否存在
- `resolve_agent_name()`: mode ID → agent profile name
- `to_session_modes()`: 转换为 ACP `SessionMode` 列表
- `to_session_mode_state()`: 构建当前 mode 状态

每个 ACP SessionMode 与 Loom AgentProfile 1:1 对应。

### `goal_runner.rs` — Goal 模式

当用户在 IDE 中输入 `/goal <description>` 时触发。

- 创建 `GoalRunner` 实例
- 通过 `LoomTool` 桥接事件回 IDE
- 支持取消（CancellationToken）
- 返回 task_id 和执行结果

### `session_config_store.rs` — 配置持久化

基于 SQLite 的 session 配置存储。

- `set()`: 保存配置项（key-value）
- `get_all()`: 获取 session 所有配置
- `copy_config()`: 复制配置（用于 fork）

### `last_model.rs` — 最近模型记忆

记住上次使用的模型，跨 session 保持。

### `terminal.rs` — 终端管理

管理 ACP 终端会话的生命周期。

### `logging.rs` — 日志系统

延迟初始化的日志系统：

- 首次调用 `init_logging()` 时初始化
- 支持 session working_folder 下的日志路径
- 支持文件轮转（daily/hourly/minutely）
- 支持格式切换（text/json）

## 协议映射

### Initialize

```
Client → Agent: InitializeRequest { protocol_version, client_capabilities, implementation }
Agent → Client: InitializeResponse { protocol_version, agent_info, agentCapabilities }
```

Agent 声明能力:
- `loadSession: true`
- `sessionCapabilities.list: {}`
- `sessionCapabilities.fork: {}`
- `promptCapabilities: { embeddedContext, image, audio }`

### Session/New

```
Client → Agent: NewSessionRequest { cwd, mcp_servers }
Agent → Client: NewSessionResponse { session_id, modes, config_options }
```

- 生成唯一 session_id
- 加载上次使用的模型
- 返回可用 modes 和 models 列表

### Session/Prompt

```
Client → Agent: PromptRequest { session_id, content_blocks }
Agent → Client: SessionNotification { session_id, session_update } (多次)
Agent → Client: PromptResponse { stop_reason }
```

### Session/Cancel

```
Client → Agent: CancelNotification { session_id }
Agent → Client: PromptResponse { stop_reason: Cancelled }
```

### Session/Fork

```
Client → Agent: ForkSessionRequest { session_id, cwd }
Agent → Client: ForkSessionResponse { session_id, modes, config_options }
```

复制源 session 的 config（不复制历史）。

### Session/Load

```
Client → Agent: LoadSessionRequest { session_id, cwd, mcp_servers }
Agent → Client: SessionNotification (历史消息重放)
Agent → Client: LoadSessionResponse { config_options, modes }
```

从 SQLite checkpoint 加载历史并通过 `session/update` 发送给客户端。

### Session/List

```
Client → Agent: ListSessionsRequest { cwd?, cursor? }
Agent → Client: ListSessionsResponse { sessions, nextCursor? }
```

查询 SQLite checkpoints 表获取所有 session 信息。分页暂未实现。

## 依赖版本状态

| 依赖 | 当前版本 | 最新版本 | 状态 |
|------|----------|----------|------|
| `agent-client-protocol` | 0.11.1 | 0.12.1 | ⚠️ 落后 |
| `agent-client-protocol-schema` | (跟随) | 0.13.2 | ⚠️ 落后 |

### 0.11.1 → 0.12.x 主要变更

- Schema 重构：所有类型标记 `#[non_exhaustive]`，enum 变体改为 tuple variant + 独立 struct
- 新增 stable: `session/close`, `session/resume`
- 新增 unstable features: `session_delete`, `session_usage`, `logout`, `mcp_over_acp`, `session_additional_directories`
- **升级风险**: 大量编译错误需要适配

### 当前 Workaround

由于 `agent-client-protocol` 0.11.x 的类型是 `#[non_exhaustive]`，多处代码通过 `serde_json::to_value()` → 修改 JSON → `serde_json::from_value()` 的方式构造响应对象。升级到 0.12.x 后部分 workaround 可能不再需要。

## 文件结构

```
loom-acp/
├── Cargo.toml
├── src/
│   ├── main.rs              # CLI 入口，PID 管理，SIGHUP 处理
│   ├── lib.rs               # run_stdio_loop()，JSON-RPC 请求注册
│   ├── agent.rs             # LoomAcpAgent，核心协议处理
│   ├── session.rs           # SessionStore，session 状态管理
│   ├── content.rs           # ContentBlock → UserContent 解析
│   ├── stream_bridge.rs     # Loom event → ACP SessionUpdate 桥接
│   ├── client_capabilities.rs # 客户端能力检测
│   ├── client_methods.rs    # 客户端方法调用实现
│   ├── agent_registry.rs    # Agent profile → ACP mode 映射
│   ├── goal_runner.rs       # /goal 命令处理
│   ├── session_config_store.rs # session 配置持久化 (SQLite)
│   ├── last_model.rs        # 最近使用模型记忆
│   ├── terminal.rs          # 终端会话管理
│   ├── logging.rs           # 延迟日志初始化
│   ├── protocol.rs          # 协议文档（rustdoc）
│   └── tools/
│       ├── mod.rs            # 工具注册
│       ├── client_bridge.rs  # 客户端桥接 trait
│       ├── fs_tools.rs       # 文件系统工具
│       └── terminal_executor.rs # 终端执行器
├── tests/                    # 测试
│   ├── common/               # 测试工具
│   ├── e2e/                  # 端到端测试
│   └── mocks/                # Mock 实现
└── docs/                     # 文档
```

## 关键设计决策

1. **Session ID = Thread ID**: ACP session_id 与 Loom thread_id 1:1 对应，保证多轮对话和 checkpointer 一致性
2. **Stdio Only**: 仅通过 stdio 通信，无额外 server 或 port
3. **serde JSON workaround**: 因为 `#[non_exhaustive]` 类型限制，多处通过 JSON 中间表示构造响应
4. **Local Bash Executor**: 使用 `LocalCommandExecutor` 在本地执行命令（ACP terminal 当前禁用）
5. **Config Persistence**: Session 配置通过 SQLite 持久化，支持跨重启保持
6. **Goal Mode**: 通过 `/goal` 命令触发，使用 `task_core::TaskDb` 进行任务管理
