# Bash 工具通过 ACP 在外部调用命令 — 逻辑梳理

## 架构总览

```
┌─────────────────────────────────────────────────────────┐
│                      LLM Agent                          │
│                   (调用 bash tool)                       │
└──────────────────────┬──────────────────────────────────┘
                       │ BashTool.call(args, ctx)
                       ▼
┌──────────────────────────────────────────────────────────┐
│  BashTool (loom/src/tools/bash/mod.rs)                   │
│  ┌────────────────────────────────────────────────────┐  │
│  │  executor.execute(command, workdir, timeout, ...)  │  │
│  └────────────┬───────────────────────────────────────┘  │
└───────────────┼──────────────────────────────────────────┘
                │
       ┌────────┴──────────┐
       ▼                   ▼
  Client 支持 terminal?    Client 不支持
       │                   │
       ▼                   ▼
┌──────────────┐   ┌───────────────────┐
│ AcpBridge    │   │ Terminal          │
│ Command      │   │ Command           │
│ Executor     │   │ Executor          │
│ (远程 ACP)    │   │ (本地 TerminalMgr)│
└──────┬───────┘   └───────┬───────────┘
       │                   │
       ▼                   ▼
┌──────────────┐   ┌───────────────────┐
│ AcpClient    │   │ TerminalManager   │
│ Bridge       │   │ (spawn 子进程)     │
│ (mpsc channel│   └───────────────────┘
│  → ACP 协议)  │
└──────┬───────┘
       │ agent_client_protocol::Client
       ▼
┌──────────────┐
│ IDE / Editor │
│ (ACP Client) │
│ 本地执行命令   │
└──────────────┘
```

---

## 1. 入口：BashTool

**文件**: `loom/src/tools/bash/mod.rs`

BashTool 是工具层统一入口。它持有 `CommandExecutor` trait 对象，通过依赖注入决定命令在本地还是远程执行。

```rust
pub struct BashTool {
    working_folder: Option<Arc<PathBuf>>,
    executor: Arc<dyn CommandExecutor>,  // 核心：策略模式
}
```

`call()` 方法（:102-135）解析参数后，直接委托给 executor：

```rust
self.executor.execute(command, working_dir, timeout_ms, vec![], ctx).await
```

---

## 2. 执行器选择

**文件**: `loom-acp/src/agent.rs:705-716`

关键决策点在 Agent 的 prompt 处理中，根据 Client 的 `terminal` 能力选择执行器：

```rust
bash_executor: {
    let caps = self.client_capabilities.read().unwrap();
    if caps.supports_terminal() {
        // ✅ Client 支持 terminal → 通过 ACP 协议远程执行
        Some(Arc::new(AcpBridgeCommandExecutor::new()))
    } else {
        // ❌ Client 不支持 → 使用本地 TerminalManager 执行
        Some(Arc::new(TerminalCommandExecutor::new(self.terminal_mgr.clone())))
    }
}
```

---

## 3. 路径 A：ACP 远程执行 — AcpBridgeCommandExecutor

**文件**: `loom-acp/src/tools/terminal_executor.rs:120-229`

当 IDE/Client 声明了 `terminal` 能力时使用。通过 ACP 协议将命令发送到 Client 端执行。

### 执行流程

```
1. 从 ctx 提取 acp_session_id
2. 获取全局 ClientBridge（AcpClientBridge）
3. bridge.terminal_create() → 返回 terminal_id
4. bridge.terminal_wait_for_exit()（支持超时 + kill）
5. bridge.terminal_output() → 获取输出
6. bridge.terminal_release() → 释放资源
```

### 关键代码

```rust
// 1. 获取 bridge（全局单例，连接 ACP Client）
let bridge = crate::tools::get_client_bridge().await?;

// 2. 创建终端（ACP: terminal/create）
let terminal_id = bridge.terminal_create(session_id, &shell, args, env, cwd, None).await?;

// 3. 等待退出（ACP: terminal/wait_for_exit），带超时控制
tokio::select! {
    result = bridge.terminal_wait_for_exit(session_id, &terminal_id) => { ... }
    _ = tokio::time::sleep(Duration::from_millis(timeout)) => {
        bridge.terminal_kill(session_id, &terminal_id).await;
        bridge.terminal_release(session_id, &terminal_id).await;
    }
}

// 4. 获取输出（ACP: terminal/output）
let output = bridge.terminal_output(session_id, &terminal_id).await?;

// 5. 释放（ACP: terminal/release）
bridge.terminal_release(session_id, &terminal_id).await?;
```

---

## 4. AcpClientBridge — 消息桥接层

**文件**: `loom-acp/src/tools/client_bridge.rs`

### 设计

`AcpClientBridge` 是 `ClientBridgeTrait` 的实现，内部使用 **mpsc channel** 将请求序列化到单个 tokio task 中执行（确保 `!Send` 的 ACP Client 能在 `spawn_local` 中运行）。

```
调用方 (AcpBridgeCommandExecutor)
    │
    │  bridge.terminal_create(...)
    ▼
AcpClientBridge (发送 BridgeRequest 到 mpsc channel)
    │
    │  BridgeRequest::TerminalCreate { reply }
    ▼
spawn_local task (接收请求)
    │
    │  client_methods::terminal_create(client, session_id, ...)
    ▼
agent_client_protocol::Client::create_terminal(request)  ← ACP SDK
    │
    ▼
IDE / Editor (ACP 协议通信)
```

### 全局单例管理

```rust
static GLOBAL_BRIDGE: OnceLock<BridgeStore> = OnceLock::new();

pub async fn set_client_bridge(bridge: Arc<dyn ClientBridgeTrait>) { ... }
pub async fn get_client_bridge() -> Result<Arc<dyn ClientBridgeTrait>, String> { ... }
```

Bridge 在 ACP 连接建立时（`agent.rs` 初始化）通过 `set_client_bridge` 注入。

---

## 5. ACP Client Methods — 协议层

**文件**: `loom-acp/src/client_methods.rs`

每个方法封装 ACP SDK 的请求/响应，将业务参数转为 ACP 协议类型：

| 方法 | ACP SDK 调用 | 说明 |
|------|-------------|------|
| `terminal_create()` | `client.create_terminal(req)` | 创建终端，返回 terminal_id |
| `terminal_output()` | `client.terminal_output(req)` | 获取当前输出 |
| `terminal_wait_for_exit()` | `client.wait_for_terminal_exit(req)` | 等待命令完成 |
| `terminal_kill()` | `client.kill_terminal(req)` | 终止命令 |
| `terminal_release()` | `client.release_terminal(req)` | 释放终端 |

`terminal_create()` 示例 (`client_methods.rs:57-97`):

```rust
pub async fn terminal_create(
    client: &dyn Client,
    session_id: &SessionId,
    command: &str,
    args: Vec<String>,
    env: Vec<(String, String)>,
    cwd: Option<String>,
    output_byte_limit: Option<u64>,
) -> Result<String, String> {
    let mut request = CreateTerminalRequest::new(session_id.clone(), command);
    if !args.is_empty() { request = request.args(args); }
    if !env.is_empty() { request = request.env(env.into_iter()...); }
    if let Some(dir) = cwd { request = request.cwd(PathBuf::from(dir)); }

    let response = client.create_terminal(request).await
        .map_err(|e| format!("terminal/create error: {:?}", e))?;
    Ok(response.terminal_id.to_string())
}
```

---

## 6. 路径 B：本地执行 — TerminalCommandExecutor

**文件**: `loom-acp/src/tools/terminal_executor.rs:12-118`

当 Client 不支持 terminal 时，使用本地的 `TerminalManager` 管理进程生命周期。

```rust
// 1. 创建终端（本地 spawn 子进程）
let terminal_id = self.terminal_mgr.create_terminal(shell, args, cwd, env, None).await?;

// 2. 等待退出（带超时）
tokio::select! {
    status = self.terminal_mgr.wait_for_exit(&terminal_id) => { ... }
    _ = tokio::time::sleep(...) => {
        self.terminal_mgr.kill(&terminal_id).await;
    }
}

// 3. 获取输出
let (output, truncated, status) = self.terminal_mgr.get_output(&terminal_id).await;

// 4. 释放
self.terminal_mgr.release(&terminal_id).await;
```

---

## 7. TerminalManager — 本地进程管理

**文件**: `loom-acp/src/terminal.rs`

实现 ACP 协议定义的完整终端生命周期，但命令在本地执行。

### 数据结构

```rust
pub struct TerminalSession {
    pub terminal_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub output_byte_limit: Option<u64>,
    pub status: TerminalStatus,       // Running | Completed | Killed | Released
    pub output_buffer: String,
    pub truncated: bool,
}
```

### 核心机制

- **进程 spawn**: `tokio::process::Command` 创建子进程，pipe stdout/stderr
- **输出读取**: 异步 reader task 循环 `read()` → 追加到 `output_buffer`，支持 `output_byte_limit` 截断
- **退出监听**: 独立 task `child.wait()` → 更新 status → `Notify` 唤醒等待者
- **kill**: 优先 `child.kill()`，回退到 `libc::kill(pid, SIGKILL)`（或 Windows `taskkill`）
- **release**: 先 kill 再标记 Released

---

## 8. 第三条路径：纯本地 — LocalCommandExecutor

**文件**: `loom/src/tools/bash/executor.rs`

`BashTool::new()` 默认使用的执行器（非 ACP 场景），直接 `spawn` shell 子进程，不经过 TerminalManager。

```rust
// BashTool::new() → LocalCommandExecutor
// BashTool::with_working_folder() → LocalCommandExecutor
// BashTool::with_executor() → 自定义 executor
```

---

## 总结对比

| 维度 | LocalCommandExecutor | TerminalCommandExecutor | AcpBridgeCommandExecutor |
|------|---------------------|------------------------|--------------------------|
| 执行位置 | 本地直接 spawn | 本地 TerminalManager | 远程 IDE (ACP 协议) |
| 使用场景 | CLI 独立运行 | ACP 模式但 Client 无 terminal | ACP 模式且 Client 支持 terminal |
| 进程管理 | tokio Command | TerminalManager | ACP Client |
| 输出获取 | pipe read_to_end | output_buffer + Notify | ACP terminal/output |
| 超时处理 | tokio::select! | tokio::select! + kill | tokio::select! + kill + release |
| 取消支持 | watch channel kill | TerminalManager.kill | ACP terminal/kill |
| 文件位置 | `loom/src/tools/bash/executor.rs` | `loom-acp/src/tools/terminal_executor.rs` | `loom-acp/src/tools/terminal_executor.rs` |

### 完整调用链（ACP 远程路径）

```
LLM 调用 bash tool
  → BashTool.call()
    → AcpBridgeCommandExecutor.execute()
      → get_client_bridge() [全局单例]
      → AcpClientBridge.terminal_create()
        → mpsc send → spawn_local task
          → client_methods::terminal_create()
            → agent_client_protocol::Client::create_terminal()
              → JSON-RPC "terminal/create" → IDE
      → AcpClientBridge.terminal_wait_for_exit()
        → ... → JSON-RPC "terminal/wait_for_exit" → IDE
      → AcpClientBridge.terminal_output()
        → ... → JSON-RPC "terminal/output" → IDE
      → AcpClientBridge.terminal_release()
        → ... → JSON-RPC "terminal/release" → IDE
  ← ToolCallContent::text(output)
```

---

## ACP 远程路径详细说明

### 核心思想

Agent 侧不直接执行 shell 命令，而是通过 ACP 协议委托给 IDE/Editor 端执行。Agent 通常运行在远程/沙箱环境中，没有本地终端可用。

---

### AcpBridgeCommandExecutor.execute() 逐步拆解

**文件**: `loom-acp/src/tools/terminal_executor.rs:130-229`

**① 提取 session_id**（:138-147）

```rust
let session_id = ctx.and_then(|c| c.acp_session_id.as_deref()).unwrap_or("default");
```

`session_id` 标识当前 ACP 会话，从 `ToolCallContext` 中获取。它会贯穿所有 JSON-RPC 请求，IDE 端用它来关联请求和会话上下文。若未设置，回退到 `"default"` 并输出 warn 日志。

**② 获取全局 Bridge 单例**（:166-171）

```rust
let bridge = crate::tools::get_client_bridge().await?;
```

`AcpClientBridge` 是全局单例（`OnceLock<RwLock<Option<Arc<dyn ClientBridgeTrait>>>>`），在 ACP 连接建立时通过 `set_client_bridge()` 注入。全局单例确保所有工具共享同一个到 IDE 的连接。

**③ 创建终端**（:175-181）

```rust
let terminal_id = bridge.terminal_create(session_id, &shell, args, env, cwd, None).await?;
```

向 IDE 请求创建一个终端进程，返回 `terminal_id`。这个 ID 由 IDE 分配，后续所有操作都用它标识这个终端。

**④ 等待退出（带超时）**（:185-208）

```rust
tokio::select! {
    result = bridge.terminal_wait_for_exit(session_id, &terminal_id) => {
        let _ = result;
    }
    _ = tokio::time::sleep(Duration::from_millis(timeout)) => {
        let _ = bridge.terminal_kill(session_id, &terminal_id).await;
        let _ = bridge.terminal_release(session_id, &terminal_id).await;
        return Err(ToolSourceError::Transport("Command timed out".into()));
    }
}
```

使用 `tokio::select!` 实现超时控制。如果超时，先 kill 再 release，然后返回超时错误。如果正常退出，继续获取输出。

**⑤ 获取输出**（:210-216）

```rust
let output = bridge.terminal_output(session_id, &terminal_id).await?;
```

一次性获取终端的所有 stdout+stderr 输出。

**⑥ 释放资源**（:218）

```rust
let _ = bridge.terminal_release(session_id, &terminal_id).await;
```

通知 IDE 释放这个终端的资源。用 `let _ =` 忽略错误，因为即使 release 失败也不影响返回结果。

**⑦ 返回结果**（:221-227）

```rust
if output.output.is_empty() {
    Ok(ToolCallContent::text("(no output)"))
} else {
    Ok(ToolCallContent::text(output.output))
}
```

---

### AcpClientBridge — 消息桥接层详解

**文件**: `loom-acp/src/tools/client_bridge.rs:134-375`

这是整个流程中最精巧的设计。它解决的问题：`agent_client_protocol::Client` **不是 `Send`**（可能包含非线程安全的 IO 资源），但 `CommandExecutor` 是 `Send + Sync` 的 trait，会在多个 tokio task 间传递。

**解决方案：mpsc channel + spawn_local（Actor 模式）**

```rust
pub struct AcpClientBridge {
    tx: mpsc::Sender<BridgeRequest>,  // 发送端是 Send 的
}
```

- `AcpClientBridge` 本身只持有一个 `mpsc::Sender`（是 `Send + Sync` 的），可以安全地跨 task 使用
- 真正的 `Client` 被 `spawn_local` task 持有，永远不离开那个 task
- 请求通过 channel 序列化，响应通过 `oneshot` channel 返回

**每个方法的标准模式**（以 `terminal_create` 为例，:283-306）：

```rust
async fn terminal_create(&self, ...) -> Result<String, String> {
    // 1. 创建 oneshot channel 用于接收响应
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

    // 2. 构造请求枚举，附带 reply sender
    self.tx.send(BridgeRequest::TerminalCreate {
        session_id, command, args, env, cwd, output_byte_limit,
        reply: reply_tx,
    }).await.map_err(|_| "bridge channel closed")?;

    // 3. 等待 spawn_local task 处理完毕，通过 oneshot 返回结果
    reply_rx.await.map_err(|_| "bridge response dropped")?
}
```

**spawn_local task 的处理循环**（:141-240）：

```rust
tokio::task::spawn_local(async move {
    while let Some(req) = rx.recv().await {
        match req {
            BridgeRequest::TerminalCreate {
                session_id, command, args, env, cwd, output_byte_limit, reply,
            } => {
                // 在这个 task 内调用 client_methods（直接使用 !Send 的 Client）
                let result = crate::client_methods::terminal_create(
                    client.as_ref(),  // Arc<C> 在这里解引用
                    &SessionId::new(&*session_id),
                    &command, args, env, cwd, output_byte_limit,
                ).await;
                let _ = reply.send(result);  // 通过 oneshot 返回给调用方
            }
            // ... 其他变体同理
        }
    }
});
```

---

### client_methods — 协议参数适配层

**文件**: `loom-acp/src/client_methods.rs`

每个函数负责将业务参数转换为 ACP 协议的 Request 类型，并处理 Response。所有方法都用 `.map_err(|e| format!(...))` 统一把 ACP SDK 错误转为 `String` 错误。

**terminal_create**（:56-96）：

```
输入：command, args, env, cwd, output_byte_limit
  → 构建 CreateTerminalRequest（Builder 模式）
  → client.create_terminal(request).await
  → JSON-RPC 请求发送 → IDE 处理 → JSON-RPC 响应返回
输出：terminal_id (String)
```

**terminal_output**（:98-131）：

```
输入：session_id, terminal_id
  → 构建 TerminalOutputRequest
  → client.terminal_output(request).await
输出：TerminalOutput { output, truncated, exit_status }
```

**terminal_wait_for_exit**（:133-162）：

```
输入：session_id, terminal_id
  → 构建 WaitForTerminalExitRequest
  → client.wait_for_terminal_exit(request).await
输出：TerminalExitResult { exit_code, signal }
```

---

### agent_client_protocol::Client — JSON-RPC 通信层

这是外部依赖（`agent-client-protocol` crate）。`Client` trait 的实现通过 **stdio 上的 JSON-RPC 2.0** 与 IDE 通信：

```
Agent 进程 (stdout) ──JSON-RPC──→ IDE
Agent 进程 (stdin)  ←─JSON-RPC── IDE
```

每个方法调用对应一个 JSON-RPC 方法：

| ACP SDK 方法 | JSON-RPC Method |
|-------------|-----------------|
| `create_terminal()` | `terminal/create` |
| `wait_for_terminal_exit()` | `terminal/wait_for_exit` |
| `terminal_output()` | `terminal/output` |
| `kill_terminal()` | `terminal/kill` |
| `release_terminal()` | `terminal/release` |

---

### 完整数据流图（含 channel 细节）

```
LLM 调用 bash tool
  │
  ▼
BashTool.call()
  │ 解析参数
  ▼
AcpBridgeCommandExecutor.execute()
  │
  ├──① ctx.acp_session_id → session_id
  │
  ├──② get_client_bridge() → Arc<dyn ClientBridgeTrait>
  │     └── GLOBAL_BRIDGE (OnceLock) → RwLock → Arc<AcpClientBridge>
  │
  ├──③ bridge.terminal_create(session_id, shell, args, env, cwd, None)
  │     │
  │     │  AcpClientBridge.terminal_create()
  │     │    ├── oneshot::channel() → (reply_tx, reply_rx)
  │     │    ├── mpsc.send(TerminalCreate { ..., reply: reply_tx })
  │     │    │     │
  │     │    │     ▼  spawn_local task (持有 Arc<Client>)
  │     │    │       ├── rx.recv() → BridgeRequest::TerminalCreate
  │     │    │       ├── client_methods::terminal_create(client, ...)
  │     │    │       │     ├── CreateTerminalRequest::new(session_id, command)
  │     │    │       │     ├── request.args(args).env(env).cwd(cwd)
  │     │    │       │     └── client.create_terminal(request).await
  │     │    │       │           └── JSON-RPC "terminal/create" → IDE
  │     │    │       │               IDE 创建子进程，返回 { terminal_id }
  │     │    │       │           ← JSON-RPC Response
  │     │    │       └── reply_tx.send(Ok(terminal_id))
  │     │    │
  │     │    └── reply_rx.await → Ok(terminal_id)
  │     │
  │     ← terminal_id
  │
  ├──④ tokio::select! {
  │     bridge.terminal_wait_for_exit(session_id, terminal_id)
  │       └── [同上 channel 路径]
  │         └── client.wait_for_terminal_exit(request)
  │           └── JSON-RPC "terminal/wait_for_exit" → IDE
  │               IDE 等待子进程退出，返回 { exit_code, signal }
  │     OR
  │     tokio::time::sleep(timeout) → kill + release → Error
  │   }
  │
  ├──⑤ bridge.terminal_output(session_id, terminal_id)
  │     └── [同上 channel 路径]
  │       └── client.terminal_output(request)
  │         └── JSON-RPC "terminal/output" → IDE
  │             IDE 读取子进程 stdout/stderr，返回 { output, truncated }
  │     ← TerminalOutput { output, truncated, exit_status }
  │
  ├──⑥ bridge.terminal_release(session_id, terminal_id)
  │     └── [同上 channel 路径]
  │       └── JSON-RPC "terminal/release" → IDE
  │           IDE 释放终端资源
  │
  └──⑦ Ok(ToolCallContent::text(output.output))
        或 Ok(ToolCallContent::text("(no output)"))
```

---

### 关键设计决策

| 设计点 | 选择 | 原因 |
|--------|------|------|
| 执行器选择 | 运行时根据 Client 能力动态决定 | 同一套代码支持 CLI 和 ACP 两种模式 |
| Bridge 全局单例 | `OnceLock<RwLock<Option<...>>>` | 整个进程只有一个 ACP 连接，所有工具共享 |
| mpsc + spawn_local | Actor 模式隔离 !Send 的 Client | Client 可能持有非线程安全的 IO 资源，不能跨 task |
| oneshot channel 响应 | 每次调用创建一对 oneshot | 请求-响应 1:1 映射，支持并发调用 |
| 超时处理 | `tokio::select!` + kill + release | 防止命令永久挂起，超时后确保清理资源 |
| release 忽略错误 | `let _ = bridge.terminal_release(...)` | 释放失败不影响已获取的输出结果 |
| session_id 传播 | ToolCallContext → Executor → JSON-RPC | IDE 端需要知道是哪个会话的请求 |
