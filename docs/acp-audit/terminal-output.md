# ACP 协议审计：terminal-output

## 协议规范

`terminal-output` ACP 协议方法处理 CLI 和 ACP bridge 之间的终端输出流式传输。它允许 agent 通过 ACP 消息传递层捕获、转发和中继终端命令输出，从而实现远程执行可见性和输出日志记录。

## 实现状态

**部分实现** — ACP 协议管道（请求/响应结构体、trait 方法、`send_request` 调用、能力检测）是真实且完整的。然而，消费 `terminal_output` 方法的 `AcpBridgeCommandExecutor` 从未在 `agent.rs` 中接入；agent 始终使用 `LocalCommandExecutor`。`terminal/output` ACP 方法已实现但未在运行系统中主动使用。

## 实现细节

### Client-Side 协议方法

**文件：** `apps/acp/src/client_methods.rs:113-147`

`terminal_output` 的请求/响应结构体和 client 方法包装器：

```rust
// Request struct and client method implementation for terminal_output
```

### Client Bridge

**文件：** `apps/acp/src/tools/client_bridge.rs:3-8`

Bridge 模块导入和初始化。

**文件：** `apps/acp/src/tools/client_bridge.rs:39-43`

`terminal_output` 的 bridge 方法分派。

**文件：** `apps/acp/src/tools/client_bridge.rs:190-207`

`terminal_output` 的 `send_request` 调用点 — 这是活动的 ACP 调用路径。

**文件：** `apps/acp/src/tools/client_bridge.rs:292-298`

`terminal_output` 的 bridge 响应处理。

### 客户端能力

**文件：** `apps/acp/src/client_capabilities.rs:20`

`terminal_output` 的能力注册。

**文件：** `apps/acp/src/client_capabilities.rs:54-57`

能力特性标志检查。

### Executor 实现

**文件：** `apps/acp/src/tools/terminal_executor.rs:167`

`AcpBridgeCommandExecutor` 定义 — 通过 ACP bridge 调用 `terminal_output` 的 executor。

**文件：** `apps/acp/src/tools/terminal_executor.rs:175-310`

完整的 executor 实现，包括 `terminal_output` 方法消费。

### 接入差距（严重）

**文件：** `apps/acp/src/agent.rs:926-929`

```rust
// 硬编码 LocalCommandExecutor — AcpBridgeCommandExecutor 从未实例化
```

这是根本原因：agent 构造硬编码了 `LocalCommandExecutor`，使 `AcpBridgeCommandExecutor` 实际上成为死代码。

## 实现方式

Loom 的 `terminal_output` 实现遵循标准 ACP bridge 模式：

1. **能力检测**：`client_capabilities.rs` 将 `terminal_output` 注册为受支持的 ACP 能力。
2. **Client Bridge**：`client_bridge.rs` 提供 `send_request` 机制以通过 ACP 消息总线调用 `terminal_output`。
3. **Executor 层**：`terminal_executor.rs` 定义 `AcpBridgeCommandExecutor` — 一种通过 `send_request` 通过 ACP bridge 路由终端输出的 executor。
4. **Agent 接入**：理论上，`agent.rs` 将实例化 `AcpBridgeCommandExecutor` 并将其注入到 agent 中。实践中，它使用 `LocalCommandExecutor`。

架构是合理的 — 管道完整，遵循 Loom 约定。问题纯粹在接入层。

## 差距与问题

### 严重：死代码 — `AcpBridgeCommandExecutor` 从未实例化

- **位置**：`apps/acp/src/agent.rs:926-929`
- **问题**：agent 构造硬编码了 `LocalCommandExecutor`，完全绕过 `AcpBridgeCommandExecutor`。
- **影响**：整个 ACP `terminal_output` 代码路径是死代码。即使 ACP bridge 可用，所有终端输出也会通过 `LocalCommandExecutor` 流式传输。
- **所需修复**：在 agent 构造中将 `LocalCommandExecutor` 实例化替换为 `AcpBridgeCommandExecutor`，由能力检查保护。

### 测试差距

- **问题**：没有测试直接执行 `terminal_output` 的 ACP `send_request` 路径。
- **当前覆盖**：仅存在本地 `TerminalManager` 测试。
- **缺失**：验证 `AcpBridgeCommandExecutor` 正确调用具有正确请求结构体的 `send_request` 并处理响应的集成或 e2e 测试。

### 无活动 Fallback

- **问题**：如果接入 `AcpBridgeCommandExecutor` 且 ACP bridge 不可用，则没有明显的回退到 `LocalCommandExecutor` 的方法。
- **建议**：考虑 executor 链或基于能力的路由，首先尝试 ACP，然后回退到本地。

## 验证

通过跟踪从能力注册到 client bridge 到 executor 实例化的完整调用链进行对抗性验证：

- **已确认文件**：10 个文件/位置已验证包含真实实现代码。
- **分析准确性**：已确认高置信度。
- **关键发现**：先前的审计引用了测试基础设施差距（空 mock，无 e2e 测试），但错过了关键的接入差距：`AcpBridgeCommandExecutor` 已定义并导出，但从未在 `agent.rs` 中实例化。
- **结论**：部分实现 — 协议管道完整且正确；激活路径已损坏。

## 总结

`terminal-output` ACP 协议**部分实现**。协议管道（请求/响应类型、能力注册、client bridge `send_request`、executor 定义）完整且正确遵循 Loom 的架构模式。然而，`apps/acp/src/agent.rs:926-929` 处的接入差距意味着 ACP 路径在运行系统中永远不会被激活。

**建议：**
1. 将 `AcpBridgeCommandExecutor` 接入到 `agent.rs:926-929` 处的 agent 构造中，由能力检测保护。
2. 添加一个集成测试，验证 `terminal_output` 的 ACP `send_request` 路径端到端执行。
3. 实现 fallback 机制，以便在 ACP bridge 不可用时使用 `LocalCommandExecutor`。
4. 在声明完整实现之前添加缺失的测试覆盖。

---

## 实现指南

### 当前实现摘要

```rust
// apps/acp/src/agent.rs:926-929（接入差距）
pub async fn new(config: AgentConfig) -> Result<Self, AgentError> {
    // ❌ 硬编码 LocalCommandExecutor
    let command_executor: Arc<dyn CommandExecutor> = Arc::new(
        LocalCommandExecutor::new(config.cwd.clone())
    );

    // AcpBridgeCommandExecutor 已定义但从未使用
    // let bridge_executor = AcpBridgeCommandExecutor::new(client_bridge);

    Ok(Self { command_executor, ... })
}
```

### 严重差距修复：接入 AcpBridgeCommandExecutor

**问题位置：** `apps/acp/src/agent.rs:926-929`

**根因：** `AcpBridgeCommandExecutor` 在 `terminal_executor.rs:167-310` 中完整定义，但 `agent.rs` 的构造函数硬编码了 `LocalCommandExecutor`，使整个 ACP `terminal_output` 路径成为死代码。

**修复前：**
```rust
// apps/acp/src/agent.rs:926-929
pub async fn new(config: AgentConfig) -> Result<Self, AgentError> {
    // ❌ 始终使用本地执行器
    let command_executor: Arc<dyn CommandExecutor> = Arc::new(
        LocalCommandExecutor::new(config.cwd.clone())
    );
    Ok(Self { command_executor, ... })
}
```

**修复后（基于能力检测路由）：**

```rust
// apps/acp/src/agent.rs:926-929
use crate::tools::terminal_executor::{
    CommandExecutor, LocalCommandExecutor, AcpBridgeCommandExecutor,
};
use crate::client_capabilities::ClientCapabilitiesInfo;

pub async fn new(
    config: AgentConfig,
    client_caps: ClientCapabilitiesInfo,
    client_bridge: Arc<ClientBridge>,
) -> Result<Self, AgentError> {
    // ✓ 基于客户端能力选择 executor
    let command_executor: Arc<dyn CommandExecutor> = if client_caps.has_terminal_output() {
        tracing::info!("using AcpBridgeCommandExecutor (client supports terminal/output)");
        Arc::new(AcpBridgeCommandExecutor::new(
            client_bridge.clone(),
            config.cwd.clone(),
        ))
    } else {
        tracing::info!("falling back to LocalCommandExecutor (client lacks terminal/output)");
        Arc::new(LocalCommandExecutor::new(config.cwd.clone()))
    };

    Ok(Self { command_executor, ... })
}
```

**ClientCapabilitiesInfo 增强：**

```rust
// apps/acp/src/client_capabilities.rs
impl ClientCapabilitiesInfo {
    /// 检查客户端是否支持 terminal/output
    pub fn has_terminal_output(&self) -> bool {
        self.terminal
            .as_ref()
            .and_then(|t| t.output)
            .unwrap_or(false)
    }
}
```

### 设计模式：能力驱动的 Executor 选择

```text
                    Agent::new()
                         │
                         ▼
              ┌──────────────────────┐
              │ 检查 client 能力     │
              │ (ClientCapabilities) │
              └──────────────────────┘
                         │
            ┌────────────┴────────────┐
            │                         │
   terminal.output=true      terminal.output=false
            │                         │
            ▼                         ▼
   ┌──────────────────┐      ┌──────────────────┐
   │ AcpBridgeCommand │      │ LocalCommand     │
   │ Executor         │      │ Executor         │
   │ (走 ACP bridge)  │      │ (直接子进程)     │
   └──────────────────┘      └──────────────────┘
            │                         │
            ▼                         ▼
   输出经 ACP 通知传回        输出直接流式返回
   (terminal/output 通知)    (本地 stdout)
```

### 命令执行流程对比

**修复前（始终走本地）：**
```text
Agent.run_command("ls -la")
  ↓
LocalCommandExecutor.execute()
  ↓
tokio::process::Command::spawn("ls")
  ↓
Child::wait() 异步等待
  ↓
stdout 流式返回
  ↓
Agent 接收输出
```

**修复后（能力驱动）：**
```text
【场景 A：客户端支持 terminal/output】

Agent.run_command("ls -la")
  ↓
AcpBridgeCommandExecutor.execute()
  ↓
terminal_executor.rs:175-310
  ↓
client_bridge.send_request("terminal/create", ...)
  ↓
Client 创建本地进程
  ↓
Client 通过 terminal/output 通知流式返回输出
  ↓
Agent 接收输出（相同语义，不同路径）

【场景 B：客户端不支持 terminal/output】

Agent.run_command("ls -la")
  ↓
LocalCommandExecutor.execute()
  ↓
（与之前相同，本地直接执行）
```

### 测试覆盖修复

**修复前：**
- 0 个测试直接执行 `terminal_output` 的 ACP `send_request` 路径
- 仅存在本地 `TerminalManager` 测试

**修复后：**

```rust
// apps/acp/tests/e2e_mega.rs — 添加 terminal_output 集成测试

#[tokio::test]
async fn test_terminal_output_uses_acp_bridge_when_capable() {
    // 1. 创建支持 terminal/output 的客户端
    let client = TestClientBuilder::new()
        .with_capability("terminal.output", true)
        .build()
        .await?;

    // 2. 启动 agent
    let agent = client.spawn_agent().await?;

    // 3. 执行命令
    let output = agent.run_command("echo hello").await?;

    // 4. 验证：使用了 ACP bridge
    assert_eq!(output.stdout.trim(), "hello");
    assert!(client.received_request("terminal/create").await,
            "agent should use ACP bridge when client supports it");
    assert!(client.received_notification("terminal/output").await,
            "agent should receive output via terminal/output");
}

#[tokio::test]
async fn test_terminal_output_falls_back_to_local() {
    // 1. 创建不支持 terminal/output 的客户端
    let client = TestClientBuilder::new()
        .with_capability("terminal.output", false)
        .build()
        .await?;

    let agent = client.spawn_agent().await?;
    let output = agent.run_command("echo local").await?;

    // 2. 验证：使用本地执行器
    assert_eq!(output.stdout.trim(), "local");
    assert!(!client.received_request("terminal/create").await,
            "agent should NOT use ACP bridge when client lacks support");
}

#[tokio::test]
async fn test_terminal_output_streaming() {
    // 验证：ACP bridge 路径下输出仍以流式方式返回
    let client = TestClientBuilder::new()
        .with_capability("terminal.output", true)
        .build()
        .await?;
    let agent = client.spawn_agent().await?;

    // 1. 启动长输出命令
    let handle = agent.run_command_async("seq 1 1000");

    // 2. 收集流式 chunk
    let mut chunks = Vec::new();
    while let Some(chunk) = handle.next_chunk().await {
        chunks.push(chunk);
    }

    // 3. 验证：收到多个 chunk（非单次批量）
    assert!(chunks.len() > 1, "output should stream in multiple chunks");

    // 4. 验证：完整输出拼接后正确
    let full: String = chunks.iter().map(|c| c.as_str()).collect();
    assert!(full.contains("999"));
    assert!(full.contains("1000"));
}

#[tokio::test]
async fn test_terminal_output_exit_code() {
    let client = TestClientBuilder::new()
        .with_capability("terminal.output", true)
        .build()
        .await?;
    let agent = client.spawn_agent().await?;

    // 1. 成功命令
    let output = agent.run_command("true").await?;
    assert_eq!(output.exit_code, 0);

    // 2. 失败命令
    let output = agent.run_command("false").await?;
    assert_eq!(output.exit_code, 1);
    assert!(!output.success);
}
```

### 演示：完整的 terminal_output 流程

**修复前：**
```text
Client (with terminal.output capability)
  │
  ├─ initialize { terminal: { output: true } }
  │
  ↓
Agent: 硬编码 LocalCommandExecutor（忽略能力）
  │
  ├─ LocalCommandExecutor::execute("ls")
  │
  ├─ tokio::process::Command::spawn("ls")
  │
  └─ 直接返回 stdout
  │
  ↓
Client: 接收结果（但 terminal/output 通知从未触发）
  ↓
  ⚠️ 能力协商失效
```

**修复后：**
```text
Client (with terminal.output capability)
  │
  ├─ initialize { terminal: { output: true } }
  │
  ↓
Agent: 检测到 client_caps.has_terminal_output() == true
  │
  ├─ 选择 AcpBridgeCommandExecutor
  │
  ├─ AcpBridgeCommandExecutor::execute("ls")
  │
  │   ├─ client_bridge.send_request("terminal/create",
  │   │     { command: "ls", args: [], cwd: ..., env: ... })
  │   │
  │   ↓
  │   Client: 创建本地进程 ls
  │   │
  │   ├─ terminal/output 通知（多次，stdout 流式）
  │   │
  │   ├─ terminal/wait_for_exit
  │   │
  │   └─ 返回 { exit_code: 0 }
  │   │
  │   ↓
  │   Agent: 接收所有 chunk + exit_code
  │
  └─ 返回完整结果
```

**演示：JSON-RPC 序列：**

```json
// 1. Agent → Client: terminal/create
{
  "jsonrpc": "2.0",
  "id": 80,
  "method": "terminal/create",
  "params": {
    "session_id": "sess-abc",
    "command": "ls -la /tmp",
    "args": [],
    "cwd": "/home/user",
    "env": {}
  }
}

// 2. Client → Agent: 响应
{
  "jsonrpc": "2.0",
  "id": 80,
  "result": { "terminalId": "term-001" }
}

// 3. Client → Agent: terminal/output 通知（多次）
{
  "jsonrpc": "2.0",
  "method": "terminal/output",
  "params": {
    "terminalId": "term-001",
    "data": "total 12\ndrwxr-xr-x 3 user user 4096 Aug 19 10:00 .\n"
  }
}

{
  "jsonrpc": "2.0",
  "method": "terminal/output",
  "params": {
    "terminalId": "term-001",
    "data": "drwxr-xr-x 2 user user 4096 Aug 19 10:00 ..\n"
  }
}

// 4. Agent → Client: terminal/wait_for_exit
{
  "jsonrpc": "2.0",
  "id": 81,
  "method": "terminal/wait_for_exit",
  "params": { "terminalId": "term-001" }
}

// 5. Client → Agent: 响应
{
  "jsonrpc": "2.0",
  "id": 81,
  "result": { "exitCode": 0, "isRunning": false }
}
```

### 演示：fallback 行为对比

```text
【修复前】无论客户端是否支持，都走本地：

Client caps: terminal.output = true (or false)
  ↓
Agent: 硬编码 LocalCommandExecutor
  ↓
结果：永远本地执行 → 能力协商失效

【修复后】根据客户端能力路由：

【场景 A】Client caps: terminal.output = true
  ↓
Agent: 选择 AcpBridgeCommandExecutor
  ↓
Client 创建进程，通过 ACP 通知流式返回

【场景 B】Client caps: terminal.output = false (或缺失)
  ↓
Agent: 回退到 LocalCommandExecutor
  ↓
Agent 进程直接 spawn，本地流式返回
```

### 接入步骤详解

**步骤 1：扩展 `Agent::new` 签名**

```rust
// apps/acp/src/agent.rs
pub async fn new(
    config: AgentConfig,
    client_caps: ClientCapabilitiesInfo,  // ← 新增参数
    client_bridge: Arc<ClientBridge>,     // ← 新增参数
) -> Result<Self, AgentError> {
    // ...
}
```

**步骤 2：能力检测**

```rust
let command_executor: Arc<dyn CommandExecutor> = if client_caps.has_terminal_output() {
    Arc::new(AcpBridgeCommandExecutor::new(client_bridge, cwd))
} else {
    Arc::new(LocalCommandExecutor::new(cwd))
};
```

**步骤 3：调用方更新（stdio_loop）**

```rust
// apps/acp/src/stdio_loop.rs
use crate::client_capabilities::ClientCapabilitiesInfo;

pub async fn spawn_agent_from_initialize(req: InitializeRequest) -> Result<Agent, ACPError> {
    // 1. 解析客户端能力（修复 initialize 差距 3 后）
    let client_caps = ClientCapabilitiesInfo::from_request(&req.client_info, &req.capabilities)?;

    // 2. 创建 client bridge（已存在）
    let client_bridge = Arc::new(ClientBridge::new(/* ... */));

    // 3. 用能力 + bridge 构造 agent
    let agent = Agent::new(config, client_caps, client_bridge).await?;
    Ok(agent)
}
```

**步骤 4：实现 fallback 机制**

```rust
// apps/acp/src/terminal_executor.rs
impl AcpBridgeCommandExecutor {
    pub async fn execute(&self, cmd: &Command) -> Result<CommandOutput, ExecutorError> {
        match self.try_via_bridge(cmd).await {
            Ok(output) => Ok(output),
            Err(ExecutorError::BridgeUnavailable) => {
                // ✓ Fallback 到本地（如果配置允许）
                tracing::warn!("ACP bridge unavailable, falling back to local executor");
                self.local_fallback.execute(cmd).await
            }
            Err(e) => Err(e),
        }
    }
}
```

### 测试场景

在 `apps/acp/tests/e2e_mega.rs` 中添加 4 个新测试：

```rust
// 1. 上述 test_terminal_output_uses_acp_bridge_when_capable
// 2. 上述 test_terminal_output_falls_back_to_local
// 3. 上述 test_terminal_output_streaming
// 4. 上述 test_terminal_output_exit_code

#[tokio::test]
async fn test_terminal_output_capability_negotiation() {
    // 单元测试：ClientCapabilitiesInfo::has_terminal_output()
    let mut caps = ClientCapabilitiesInfo::default();
    assert!(!caps.has_terminal_output());

    caps.terminal = Some(TerminalCapabilities { output: Some(true) });
    assert!(caps.has_terminal_output());

    caps.terminal = Some(TerminalCapabilities { output: Some(false) });
    assert!(!caps.has_terminal_output());
}

#[tokio::test]
async fn test_terminal_output_error_propagation() {
    let client = TestClientBuilder::new()
        .with_capability("terminal.output", true)
        .with_terminal_error(TerminalError::ProcessNotFound)
        .build()
        .await?;
    let agent = client.spawn_agent().await?;

    let result = agent.run_command("nonexistent_command");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), AgentError::TerminalError(_)));
}
```

### 验证已修正的差距

**先前分析中标记的实现差距（已澄清）：**

| 误报差距 | 实际状态 |
|---------|---------|
| "测试基础设施差距"（空 mock，无 e2e 测试） | 这是真实差距，但**根因**是 AcpBridgeCommandExecutor 死代码 — 修复接入后，测试自然能写 |
| "AcpBridgeCommandExecutor 死代码" | **真实严重差距** — 修复后整个 ACP 路径激活 |

### 验收清单

**严重差距 — 接入 AcpBridgeCommandExecutor：**
- [ ] `agent.rs:926-929` `Agent::new` 签名扩展（增加 `client_caps` + `client_bridge`）
- [ ] `agent.rs:926-929` 实现能力驱动的 executor 选择
- [ ] `client_capabilities.rs` 添加 `has_terminal_output()` 方法
- [ ] `stdio_loop.rs` 调用方更新（传入 client_caps + bridge）
- [ ] `terminal_executor.rs` 实现 fallback 机制

**测试覆盖：**
- [ ] 添加 `test_terminal_output_uses_acp_bridge_when_capable`
- [ ] 添加 `test_terminal_output_falls_back_to_local`
- [ ] 添加 `test_terminal_output_streaming`
- [ ] 添加 `test_terminal_output_exit_code`
- [ ] 添加 `test_terminal_output_capability_negotiation`
- [ ] 添加 `test_terminal_output_error_propagation`
- [ ] 验证：6 个新测试在修复后通过

**Fallback 机制（推荐）：**
- [ ] `AcpBridgeCommandExecutor::try_via_bridge` 返回 `BridgeUnavailable` 错误
- [ ] 上层捕获错误并回退到 `LocalCommandExecutor`
- [ ] 记录回退事件以便诊断

**生产环境监控：**
- [ ] 添加 `tracing` 字段：每次 executor 选择（bridge vs local）
- [ ] 添加 metrics：bridge 调用次数、fallback 次数、失败次数
- [ ] 验证：能力协商与实际行为一致
