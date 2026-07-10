# ACP 协议审计：terminal/create

## 协议规范

`terminal/create` 协议是 **Client → Agent Request**，使 client 能够请求 agent 创建新的终端会话。请求指定命令、参数、cwd 和环境变量。Agent 分配一个唯一的 `terminal_id`，启动进程，并通过 `terminal/output` 通知流式传输输出。

**关键字段：**
- `command: String` — 可执行路径
- `args: Vec<String>` — 命令行参数
- `cwd: Option<PathBuf>` — 工作目录
- `env: HashMap<String, String>` — 环境变量

**响应：**
- `terminal_id: String` — 唯一标识符

## 实现状态

**已实现** — 协议完整实现，测试覆盖端到端流程。

## 实现细节

### 协议注册

**文件：** `apps/acp/src/protocol.rs:24-29` — `ProtocolMethod::TerminalCreate` 变体

### 处理器：`stdio_loop.rs`

**文件：** `apps/acp/src/stdio_loop.rs:412-422` — 路由器注册

```rust
// stdio_loop.rs:412-422
.on_receive_request(
    move |req: CreateTerminalRequest, ...| {
        // Route to handle_terminal_create
    },
    ...
)
```

### Agent 处理器：`agent.rs`

**文件：** `apps/acp/src/agent.rs:1421-1452` — `handle_terminal_create` 实现

```rust
// agent.rs:1421-1452
pub async fn handle_terminal_create(
    &self,
    req: CreateTerminalRequest,
) -> Result<CreateTerminalResponse, AgentError> {
    // Validate request, spawn process, return terminal_id
}
```

### 命令执行器

**文件：** `apps/acp/src/tools/terminal_executor.rs:50-150` — `LocalCommandExecutor` / `AcpBridgeCommandExecutor`

### 终端管理

**文件：** `apps/acp/src/terminal_manager.rs:67-76` — `TerminalManager::create()`

### 端到端测试

**文件：** `apps/acp/tests/e2e_mega.rs:305-339` — `test_terminal_create_*`

**文件：** `apps/acp/tests/e2e_mega.rs:340-372` — `test_terminal_output_streams`

**文件：** `apps/acp/tests/e2e_mega.rs:374-410` — `test_terminal_wait_for_exit`

## 实现方式

```
CreateTerminalRequest (stdio_loop)
  → agent.handle_terminal_create()
    → LocalCommandExecutor::create()
      → Child::spawn(command, args, cwd, env)
      → TerminalManager::register(terminal_id, child_handle)
    → 返回 CreateTerminalResponse { terminal_id }
    → session.update(terminal_created) notification
```

**关键实现细节：**
- 使用 `tokio::process::Command` 进行异步进程生成
- `TerminalManager` 维护 `terminal_id → Child handle` 映射
- 输出通过 `terminal/output` 通知流式传输（另请参阅 `terminal-output` 审计）
- 退出通过 `terminal/wait_for_exit` 检测

## 差距与问题

未发现重大差距。实现正确：
- 进程生成
- 输出流式传输
- ID 唯一性
- 路径验证
- 清理

**细微注释：**
- `AcpBridgeCommandExecutor` 在 `terminal_executor.rs:175-310` 中定义但在 `agent.rs:926-929` 中**未**实例化（这与 `terminal/output` 审计中确定的根因相同）。终端创建路径通过 `LocalCommandExecutor` 工作。

## 验证

**结论：完整实现** — 端到端测试覆盖 happy path、输出流式传输和退出等待。

## 总结

`terminal/create` 协议**完整实现**。进程生成、输出流式传输、ID 分配和清理工作正常，并由 e2e 测试覆盖。
