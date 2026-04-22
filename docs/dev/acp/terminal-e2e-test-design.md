# Terminal E2E Test Design

基于 [ACP Terminal Command Flow](./acp-terminal-command-flow.md)，对 loom-acp 的 Terminal 功能进行端到端测试设计。

## 核心架构约束

Terminal 操作（`terminal/create`、`terminal/output`、`terminal/wait_for_exit`、`terminal/kill`、`terminal/release`）
是 **Agent→Client** 方向的请求。loom-acp 作为 Agent 发起这些调用，Client（IDE）负责实际执行。

因此存在两条路径：

| 路径 | Client 声明 `terminal` 能力 | 执行器 | 命令执行位置 |
|------|---------------------------|--------|-------------|
| A | ✅ 是 | `AcpBridgeCommandExecutor` | Client 端（IDE/测试桩） |
| B | ❌ 否 | `TerminalCommandExecutor` | Agent 本地（`TerminalManager`） |

**E2E 测试视角**：测试桩（`AcpChild`）扮演 Client 角色，通过 stdin/stdout JSON-RPC 与 loom-acp 通信。

- **路径 A**：测试桩需要处理 loom-acp 发来的 `terminal/*` 请求，实际执行命令并返回结果
- **路径 B**：loom-acp 在本地执行命令，测试桩只需验证 prompt 最终响应中包含正确输出

## 测试基础设施需求

### 1. 增强 `AcpChild`：支持双向 JSON-RPC

当前 `AcpChild` 只能发送请求、读取响应/通知。路径 A 需要额外能力：

```
loom-acp (Agent)                    AcpChild (Client/测试桩)
    │                                    │
    │  session/request_permission        │
    │  ──────────────────────────────►   │  测试桩自动 grant
    │  ◄──────────────────────────────   │
    │                                    │
    │  terminal/create                   │  测试桩收到请求
    │  ──────────────────────────────►   │  实际 spawn 子进程
    │  ◄─── { terminal_id }             │  返回结果
    │                                    │
    │  terminal/output                   │
    │  ──────────────────────────────►   │  读取进程输出
    │  ◄─── { output, truncated }       │
    │                                    │
    │  terminal/wait_for_exit            │
    │  ──────────────────────────────►   │  等待进程退出
    │  ◄─── { exit_code }               │
    │                                    │
    │  terminal/release                  │
    │  ──────────────────────────────►   │  清理资源
    │  ◄─── { }                         │
```

新增组件：

- **`TerminalHandler`**：内嵌 `TerminalManager`，处理 loom-acp 发来的 `terminal/*` 请求
- **`PermissionHandler`**：自动响应 `session/request_permission`，按策略 grant/deny
- **消息分发器**：区分响应（有 `id` 无 `method`）、通知（有 `method` 无 `id`）、**Agent→Client 请求**（有 `id` 有 `method`），并分别路由

### 2. 初始化能力声明

路径 A 需要在 `initialize` 时声明 `terminal` 能力：

```json
{
  "method": "initialize",
  "params": {
    "protocolVersion": "0.10",
    "capabilities": {
      "terminal": {}
    },
    "clientInfo": { "name": "test-client", "version": "1.0.0" }
  }
}
```

路径 B 不声明 `terminal` 能力（或不包含 `terminal` 字段）。

---

## 测试用例

### Phase T1: Capability Negotiation — 能力协商

| # | 测试 | 输入 | 验证点 | 优先级 |
|---|------|------|--------|--------|
| T1.1 | `e2e_terminal_capability_advertised` | `initialize` 带 `capabilities.terminal` | agent 记录 terminal 能力；prompt 触发 bash tool 时，Agent 发送 `terminal/create` 请求给 Client（而非本地执行） | P0 |
| T1.2 | `e2e_terminal_capability_absent` | `initialize` 不带 `capabilities.terminal` | prompt 触发 bash tool 时，Agent 使用本地 `TerminalManager` 执行，不发送 `terminal/*` 请求 | P0 |
| T1.3 | `e2e_terminal_capability_changes_per_session` | 先 initialize 不带 terminal → session 1 prompt；再 initialize 带 terminal → session 2 prompt | 不同 initialize 会话的能力互相独立 | P2 |

### Phase T2: Terminal Create — 终端创建（路径 A）

| # | 测试 | Agent→Client 请求 | 验证点 | 优先级 |
|---|------|-------------------|--------|--------|
| T2.1 | `e2e_terminal_create_basic` | `terminal/create` (command=`echo hello`) | Client 收到请求；返回 `terminal_id`（非空字符串，前缀 `term-`）；后续 `terminal/output` 返回包含 "hello" 的输出 | P0 |
| T2.2 | `e2e_terminal_create_with_args` | `terminal/create` (command=`/bin/sh`, args=[`-c`, `echo hello`]) | Agent 正确拆分 command 和 args；Client 端命令执行成功 | P0 |
| T2.3 | `e2e_terminal_create_with_cwd` | `terminal/create` (command=`pwd`, cwd=`/tmp`) | 输出包含 `/tmp`（或指定的工作目录） | P1 |
| T2.4 | `e2e_terminal_create_with_env` | `terminal/create` (command=`echo $MY_VAR`, env=`{MY_VAR: "test_val"}`) | 输出包含 `test_val` | P1 |
| T2.5 | `e2e_terminal_create_session_id_consistent` | `terminal/create` 带当前 `session_id` | 请求中的 `sessionId` 与当前活跃 session 一致 | P1 |
| T2.6 | `e2e_terminal_create_invalid_command` | `terminal/create` (command=`nonexistent_command_xyz`) | Client 返回错误或非零退出码；Agent 将错误信息返回给 LLM/用户 | P1 |

### Phase T3: Terminal Output — 输出获取（路径 A）

| # | 测试 | Agent→Client 请求 | 验证点 | 优先级 |
|---|------|-------------------|--------|--------|
| T3.1 | `e2e_terminal_output_basic` | 创建终端 → `terminal/output` | 返回 `output`（String）、`truncated`（Boolean）；输出内容与命令实际输出一致 | P0 |
| T3.2 | `e2e_terminal_output_includes_stdout_stderr` | 执行 `echo out && echo err >&2` | 输出同时包含 stdout 和 stderr 内容 | P1 |
| T3.3 | `e2e_terminal_output_after_completion` | 命令完成后再调 `terminal/output` | 返回完整输出（进程已结束，不再增长） | P0 |
| T3.4 | `e2e_terminal_output_before_completion` | 长运行命令，中途调 `terminal/output` | 返回截至目前已缓冲的输出（部分输出） | P1 |
| T3.5 | `e2e_terminal_output_unknown_id` | `terminal/output` (不存在的 `terminal_id`) | 返回 JSON-RPC 错误（`NotFound` / `invalid_params`） | P1 |

### Phase T4: Terminal Wait For Exit — 等待退出（路径 A）

| # | 测试 | Agent→Client 请求 | 验证点 | 优先级 |
|---|------|-------------------|--------|--------|
| T4.1 | `e2e_terminal_wait_for_exit_success` | 执行 `echo done` → `terminal/wait_for_exit` | 返回 `exit_code: 0`，`signal: null` | P0 |
| T4.2 | `e2e_terminal_wait_for_exit_failure` | 执行 `exit 1` → `terminal/wait_for_exit` | 返回 `exit_code: 1`（或非零） | P0 |
| T4.3 | `e2e_terminal_wait_for_exit_already_exited` | 命令已结束 → `terminal/wait_for_exit` | 立即返回退出状态，不阻塞 | P1 |
| T4.4 | `e2e_terminal_wait_for_exit_unknown_id` | `terminal/wait_for_exit` (不存在的 `terminal_id`) | 返回 JSON-RPC 错误 | P1 |

### Phase T5: Terminal Kill — 终止进程（路径 A）

| # | 测试 | Agent→Client 请求 | 验证点 | 优先级 |
|---|------|-------------------|--------|--------|
| T5.1 | `e2e_terminal_kill_running` | 执行 `sleep 60` → `terminal/kill` | 进程被终止；后续 `wait_for_exit` 返回 `signal: "SIGKILL"` 或类似；`output` 仍可获取 | P0 |
| T5.2 | `e2e_terminal_kill_already_exited` | 执行 `echo done`（等结束）→ `terminal/kill` | 返回成功或幂等错误（不崩溃） | P1 |
| T5.3 | `e2e_terminal_kill_unknown_id` | `terminal/kill` (不存在的 `terminal_id`) | 返回 JSON-RPC 错误（`NotFound`） | P1 |

### Phase T6: Terminal Release — 资源释放（路径 A）

| # | 测试 | Agent→Client 请求 | 验证点 | 优先级 |
|---|------|-------------------|--------|--------|
| T6.1 | `e2e_terminal_release_after_completion` | 命令结束 → `terminal/release` | 返回成功；后续对同一 `terminal_id` 的操作返回 `NotFound` 或 `AlreadyReleased` | P0 |
| T6.2 | `e2e_terminal_release_running` | 长运行命令 → `terminal/release` | 先 kill 再释放；进程终止 | P0 |
| T6.3 | `e2e_terminal_release_unknown_id` | `terminal/release` (不存在的 `terminal_id`) | 返回 JSON-RPC 错误 | P1 |
| T6.4 | `e2e_terminal_double_release` | `terminal/release` → 再次 `terminal/release` | 第二次返回 `AlreadyReleased` 错误，不崩溃 | P1 |

### Phase T7: Full Lifecycle via Prompt — 通过 Prompt 触发完整流程

这些测试通过 `session/prompt` 间接触发 terminal 操作，需要 mock AI provider 返回 tool call 响应。

| # | 测试 | Prompt 内容 | 验证点 | 路径 | 优先级 |
|---|------|-------------|--------|------|--------|
| T7.1 | `e2e_prompt_triggers_bash_tool` | "Run `echo hello`" | 收到 `session/update` 通知含 `tool_call`（Pending）；收到 `session/request_permission`；grant 后 agent 执行 bash tool；收到 `tool_call_update`（Running → Success）；最终 `endTurn` | A | P0 |
| T7.2 | `e2e_prompt_bash_output_in_response` | "Run `echo hello`" | agent 最终响应中包含 "hello"（命令输出） | A+B | P0 |
| T7.3 | `e2e_prompt_bash_permission_denied` | "Run `echo hello`" | 收到 `request_permission` → deny → agent 收到拒绝 → `stopReason: "endTurn"`，响应说明权限被拒绝 | A | P1 |
| T7.4 | `e2e_prompt_bash_timeout` | "Run `sleep 300`" (设置短 timeout) | Agent 在 timeout 后 kill 进程；返回超时错误信息 | A+B | P1 |
| T7.5 | `e2e_prompt_bash_working_dir` | "Run `pwd` in /tmp" | 命令在指定 cwd 执行；输出包含 `/tmp` | A+B | P1 |
| T7.6 | `e2e_prompt_multiple_bash_calls` | "Run `echo one`, then `echo two`" | 两个 bash tool call 依次执行；各自输出正确 | A+B | P1 |
| T7.7 | `e2e_prompt_bash_cancel_during_execution` | 发 prompt → 命令运行中 → `session/cancel` | Agent kill 正在运行的 terminal；返回 `stopReason: "cancelled"` | A+B | P2 |

### Phase T8: Local Execution — 本地执行路径（路径 B）

Client 不声明 terminal 能力时，Agent 使用 `TerminalCommandExecutor` + 本地 `TerminalManager`。

| # | 测试 | 验证点 | 优先级 |
|---|------|--------|--------|
| T8.1 | `e2e_local_bash_echo` | prompt 触发 bash tool → 输出包含命令结果；Agent 不发送 `terminal/*` 请求给 Client | P0 |
| T8.2 | `e2e_local_bash_exit_code_nonzero` | 执行 `exit 1` → agent 响应包含错误信息 | P1 |
| T8.3 | `e2e_local_bash_env_vars` | 带 env 执行 → 输出包含环境变量值 | P1 |
| T8.4 | `e2e_local_bash_cwd` | 在指定 cwd 执行 → 输出包含正确路径 | P1 |
| T8.5 | `e2e_local_bash_output_truncation` | 产生大量输出 + `output_byte_limit` → 输出被截断，`truncated: true` | P2 |

### Phase T9: Concurrency & Edge Cases — 并发与边界

| # | 测试 | 验证点 | 优先级 |
|---|------|--------|--------|
| T9.1 | `e2e_terminal_multiple_concurrent_sessions` | 两个 session 各自 prompt 执行 bash → 输出互不干扰 | P1 |
| T9.2 | `e2e_terminal_rapid_create_release` | 快速创建并释放 10 个 terminal → 无泄漏、无 panic | P2 |
| T9.3 | `e2e_terminal_special_chars_in_command` | 执行含特殊字符的命令（`echo "hello world"`, `echo 'it\'s'`）→ 输出正确 | P2 |
| T9.4 | `e2e_terminal_unicode_output` | 执行 `echo "你好世界"` → 输出包含 unicode 字符 | P2 |
| T9.5 | `e2e_terminal_large_output` | 执行 `seq 1 10000` → 输出完整（或被 output_byte_limit 正确截断） | P2 |

---

## 实现方案

### 文件结构

```
loom-acp/tests/
├── common/
│   ├── acp_child.rs          # 增强：支持 Agent→Client 请求处理
│   ├── terminal_handler.rs   # 新增：处理 terminal/* 请求
│   ├── permission_handler.rs # 新增：处理 request_permission
│   └── mod.rs
├── e2e/
│   ├── terminal_e2e.rs       # 新增：Terminal E2E 测试（路径 A）
│   ├── terminal_local_e2e.rs # 新增：Terminal 本地执行 E2E 测试（路径 B）
│   └── mod.rs
```

### `TerminalHandler` 核心设计

```rust
pub struct TerminalHandler {
    manager: TerminalManager,
}

impl TerminalHandler {
    pub fn new() -> Self { ... }

    /// 处理 Agent 发来的 terminal/* 请求，返回 JSON-RPC 响应
    pub async fn handle_request(
        &self,
        method: &str,
        params: &Value,
    ) -> Result<Value, String> {
        match method {
            "terminal/create" => { ... }
            "terminal/output" => { ... }
            "terminal/wait_for_exit" => { ... }
            "terminal/kill" => { ... }
            "terminal/release" => { ... }
            _ => Err(format!("unknown terminal method: {}", method))
        }
    }
}
```

### `PermissionHandler` 核心设计

```rust
pub enum PermissionPolicy {
    AlwaysGrant,
    AlwaysDeny,
    Conditional(Box<dyn Fn(&str) -> bool>),  // 根据 tool_name 判断
}

pub struct PermissionHandler {
    policy: PermissionPolicy,
}

impl PermissionHandler {
    pub fn handle_request_permission(
        &self,
        params: &Value,
    ) -> Value { ... }
}
```

### `AcpChild` 消息分发增强

在读取 stdout 时，需要区分三种消息：

```rust
fn classify_message(msg: &Value) -> MessageClass {
    let has_id = msg.get("id").is_some();
    let has_method = msg.get("method").is_some();

    match (has_id, has_method) {
        (true, false)  => MessageClass::Response,        // Agent 对 Client 请求的响应
        (false, true)  => MessageClass::Notification,     // Agent→Client 通知
        (true, true)   => MessageClass::AgentRequest,     // Agent→Client 请求（需回复）
        _              => MessageClass::Unknown,
    }
}
```

新增方法：

```rust
impl AcpChild {
    /// 发送 prompt 并自动处理 Agent→Client 交互（permission + terminal）
    pub async fn prompt_with_terminal(
        &mut self,
        session_id: &str,
        text: &str,
        timeout: Duration,
        perm_policy: PermissionPolicy,
    ) -> Result<PromptResult, Box<dyn std::error::Error>> { ... }
}
```

### Mock AI Provider — Tool Call 响应

Prompt 触发 bash tool 需要 mock AI provider 返回 tool call 格式的响应：

```json
{
  "choices": [{
    "message": {
      "role": "assistant",
      "content": null,
      "tool_calls": [{
        "id": "call_1",
        "type": "function",
        "function": {
          "name": "bash",
          "arguments": "{\"command\": \"echo hello\"}"
        }
      }]
    },
    "finish_reason": "tool_calls"
  }]
}
```

后续 round 需要返回包含 tool 结果的最终响应。

---

## 与现有测试的关系

| 现有测试 | 层级 | 本设计的关系 |
|---------|------|-------------|
| `test_terminal_integration.rs` | 单元测试（`TerminalManager` 直测） | Phase T2-T6 的协议层验证是这些单元测试的 E2E 对应 |
| `session_lifecycle.rs` | E2E（协议层） | 本设计复用 `AcpChild.handshake()` 模式 |
| `initialization_detailed.rs` | E2E（协议层） | T1 复用并扩展 initialize 能力声明 |

### 测试依赖顺序

```
T1 (能力协商) → T2 (create) → T3 (output) → T4 (wait_for_exit) → T5 (kill) → T6 (release)
                                                      ↘
T7 (prompt 全流程) ← 依赖 T1-T6 全部通过
T8 (本地执行) ← 仅依赖基本 E2E 基础设施
T9 (并发/边界) ← 依赖 T7
```

---

## 优先级总结

- **P0（必须实现）**: T1.1, T1.2, T2.1, T2.2, T3.1, T3.3, T4.1, T4.2, T5.1, T6.1, T6.2, T7.1, T7.2, T8.1
- **P1（应该实现）**: T2.3, T2.4, T2.5, T2.6, T3.2, T3.4, T3.5, T4.3, T4.4, T5.2, T5.3, T6.3, T6.4, T7.3, T7.4, T7.5, T7.6, T8.2, T8.3, T8.4, T9.1
- **P2（可以实现）**: T1.3, T7.7, T8.5, T9.2, T9.3, T9.4, T9.5
