---
sidebar_position: 4
title: "Codex Shell 执行引擎"
description: "Codex 的 Shell 命令执行架构：审批、沙箱隔离与重试机制"
---

# Codex Shell 执行引擎

Codex 实现了一套多层次的 Shell 命令执行引擎，提供从工具 Handler 到沙箱隔离的完整执行链路。本文档详细解析其架构设计和代码逻辑。

## 使用场景

| 场景 | 适用性 | 说明 |
|------|--------|------|
| 受限命令执行 | ✅ 完美支持 | 通过沙箱隔离文件系统和网络访问 |
| 自动化审批 | ✅ 完美支持 | 基于策略的命令审批和缓存机制 |
| 跨平台执行 | ✅ 完美支持 | Unix (sh/zsh) 和 Windows (PowerShell) |
| 容器内执行 | ✅ 完美支持 | `container.exec` 支持容器化场景 |
| 安全敏感操作 | ✅ 完美支持 | Guardian 自动审批 + 用户交互审批 |

## 整体架构

```
Agent 请求
    │
    ▼
┌─────────────────────────────────────────────────┐
│  Handler 层 (4 个 Handler)                       │
│  shell / shell_command / local_shell / container │
└───────────────┬─────────────────────────────────┘
                │ run_exec_like()
                ▼
┌─────────────────────────────────────────────────┐
│  调度层 (shell.rs)                               │
│  环境准备 → 权限处理 → 策略决策 → 事件发射       │
└───────────────┬─────────────────────────────────┘
                │ orchestrator.run()
                ▼
┌─────────────────────────────────────────────────┐
│  编排层 (ToolOrchestrator)                       │
│  审批 → 沙箱选择 → 首次执行 → 失败重试           │
└───────────────┬─────────────────────────────────┘
                │ ShellRuntime::run()
                ▼
┌─────────────────────────────────────────────────┐
│  执行层 (ShellRuntime)                           │
│  命令构建 → 沙箱变换 → 进程执行 → 输出捕获       │
└─────────────────────────────────────────────────┘
```

## 文件结构

```
thirdparty/codex/codex-rs/core/src/tools/
├── handlers/
│   ├── shell.rs                    # 核心调度：run_exec_like() + RunExecLikeArgs
│   └── handlers/shell/
│       ├── shell_handler.rs        # "shell" Handler
│       ├── shell_command.rs        # "shell_command" Handler
│       ├── local_shell.rs          # "local_shell" Handler
│       └── container_exec.rs       # "container.exec" Handler
├── runtimes/shell.rs               # ShellRuntime：审批 + 沙箱执行
├── orchestrator.rs                 # ToolOrchestrator：审批→沙箱→执行→重试
├── sandboxing.rs                   # Approvable / Sandboxable / ToolRuntime trait
└── exec.rs                         # ExecParams + 底层进程执行
```

## Handler 层

四个 Handler 都实现 `ToolHandler` trait，分别对应不同的执行场景：

| Handler | 工具名 | freeform | 后端 | 说明 |
|---------|--------|----------|------|------|
| `ShellHandler` | `shell` | `false` | Generic | 通用 shell 执行 |
| `ShellCommandHandler` | `shell_command` | `true` | Classic/ZshFork | 支持 login shell 和 zsh fork 后端 |
| `LocalShellHandler` | `local_shell` | `false` | Generic | 本地 shell，无额外权限 |
| `ContainerExecHandler` | `container.exec` | `false` | Generic | 容器内执行 |

### Handler 接口

```rust
// shell_handler.rs:31
pub struct ShellHandler {
    options: Option<ShellToolOptions>,
}

// shell_command.rs:40
pub struct ShellCommandHandler {
    backend: ShellCommandBackend,       // Classic 或 ZshFork
    options: Option<ShellCommandHandlerOptions>,
}
```

### handle() 通用流程

每个 Handler 的 `handle()` 方法遵循相同的模式（以 `ShellHandler` 为例，`shell_handler.rs:111`）：

1. 解析 `ToolInvocation`，拆出 `session`、`turn`、`call_id`、`payload`
2. 从 `payload` 提取 JSON `arguments`
3. `resolve_workdir_base_path()` 解析工作目录
4. `parse_arguments_with_base_path()` 反序列化为 `ShellToolCallParams`
5. `to_exec_params()` 构造 `ExecParams`
6. 调用共享的 `run_exec_like()` 函数

```rust
// shell_handler.rs:111
async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
    let arguments = match payload {
        ToolPayload::Function { arguments } => arguments,
        _ => return Err(FunctionCallError::RespondToModel("unsupported payload".into())),
    };

    let cwd = resolve_workdir_base_path(&arguments, &turn.cwd)?;
    let params: ShellToolCallParams = parse_arguments_with_base_path(&arguments, &cwd)?;
    let exec_params = ShellHandler::to_exec_params(&params, turn.as_ref(), session.conversation_id);

    run_exec_like(RunExecLikeArgs {
        tool_name: ToolName::plain("shell"),
        exec_params,
        hook_command: shlex_join(&params.command),
        session, turn, tracker, call_id,
        freeform: false,
        shell_runtime_backend: ShellRuntimeBackend::Generic,
        // ...
    }).await
}
```

### 命令安全性判断

所有 Handler 通过 `is_mutating()` 判断命令是否为修改操作：

```rust
// shell_handler.rs:89
async fn is_mutating(&self, invocation: &ToolInvocation) -> bool {
    let arguments = &invocation.payload;
    serde_json::from_str::<ShellToolCallParams>(arguments)
        .map(|params| !is_known_safe_command(&params.command))
        .unwrap_or(true)
}
```

`is_known_safe_command()` 来自 `codex_shell_command` crate，维护一个只读命令白名单（如 `ls`、`cat`、`grep`、`git status` 等）。

## 调度层：run_exec_like()

`shell.rs:110` — 所有 Handler 共享的核心调度入口，接收 `RunExecLikeArgs` 参数。

### 参数结构

```rust
// shell.rs:75
struct RunExecLikeArgs {
    tool_name: ToolName,
    exec_params: ExecParams,
    hook_command: String,
    additional_permissions: Option<AdditionalPermissionProfile>,
    prefix_rule: Option<Vec<String>>,
    session: Arc<Session>,
    turn: Arc<TurnContext>,
    tracker: SharedTurnDiffTracker,
    call_id: String,
    freeform: bool,
    shell_runtime_backend: ShellRuntimeBackend,
}
```

### 执行流程

```
run_exec_like()
    │
    ├─ 1. 环境准备（:126-143）
    │     ├─ 获取 turn_environment 的文件系统
    │     ├─ 注入 dependency_env（依赖环境变量）
    │     └─ 合并显式环境变量覆盖
    │
    ├─ 2. 权限处理（:145-194）
    │     ├─ apply_granted_turn_permissions() 应用已授权的 turn 级权限
    │     ├─ normalize_and_validate_additional_permissions() 规范化额外权限
    │     └─ 策略检查：升级权限与审批策略冲突时直接拒绝
    │
    ├─ 3. apply_patch 拦截（:197-210）
    │     └─ intercept_apply_patch() 检测命令中的 patch 操作
    │
    ├─ 4. 事件发射（:212-225）
    │     └─ ToolEmitter::shell() 发出命令执行开始事件
    │
    ├─ 5. 执行策略决策（:227-244）
    │     └─ exec_policy.create_exec_approval_requirement_for_command()
    │        → ExecApprovalRequirement { Skip | NeedsApproval | Forbidden }
    │
    ├─ 6. 构造 ShellRequest（:246-261）
    │
    ├─ 7. 创建 Runtime 和 Orchestrator（:262-277）
    │     ├─ ShellRuntime::new() 或 ShellRuntime::for_shell_command(backend)
    │     └─ ToolOrchestrator::new()
    │
    ├─ 8. orchestrator.run()（:278-287）
    │
    └─ 9. 发射完成事件并返回（:288-308）
```

### ExecParams 结构

```rust
// exec.rs:84
pub struct ExecParams {
    pub command: Vec<String>,
    pub cwd: AbsolutePathBuf,
    pub expiration: ExecExpiration,
    pub capture_policy: ExecCapturePolicy,
    pub env: HashMap<String, String>,
    pub network: Option<NetworkProxy>,
    pub sandbox_permissions: SandboxPermissions,
    pub windows_sandbox_level: WindowsSandboxLevel,
    pub windows_sandbox_private_desktop: bool,
    pub justification: Option<String>,
    pub arg0: Option<String>,
}
```

## 编排层：ToolOrchestrator

`orchestrator.rs:41` — 驱动 **审批 → 沙箱选择 → 执行 → 失败重试** 的完整生命周期。

### 核心流程

```
ToolOrchestrator::run()
    │
    ├─ 1. 审批阶段（:144-214）
    │     │
    │     ├─ ExecApprovalRequirement::Skip
    │     │   ├─ strict_auto_review → 走 guardian 审批
    │     │   └─ 否则 → 直接通过
    │     │
    │     ├─ ExecApprovalRequirement::Forbidden
    │     │   └─ 直接返回 Rejected
    │     │
    │     └─ ExecApprovalRequirement::NeedsApproval
    │         ├─ PermissionRequest hooks（优先级最高）
    │         │   ├─ Allow → 通过
    │         │   └─ Deny → 拒绝
    │         ├─ Guardian 自动审批（如果启用）
    │         └─ 用户交互审批（兜底）
    │
    ├─ 2. 沙箱选择（:216-246）
    │     ├─ BypassSandboxFirstAttempt → SandboxType::None
    │     └─ SandboxManager::select_initial()
    │        根据文件系统策略、网络策略、平台自动选择
    │
    ├─ 3. 首次执行（:248-255）
    │     └─ run_attempt()
    │        ├─ begin_network_approval()
    │        ├─ ShellRuntime::run()
    │        └─ finish_network_approval()
    │
    └─ 4. 失败重试（:264-378）
          │  沙箱拒绝 (SandboxErr::Denied)
          │
          ├─ 判断是否允许升级
          │   ├─ escalate_on_failure()
          │   ├─ wants_no_sandbox_approval()
          │   └─ 审批策略检查
          │
          ├─ 重新审批（如需要）
          │
          └─ 第二次执行（SandboxType::None，无沙箱）
```

### 审批决策路径

```
request_approval()
    │
    ├─ PermissionRequest hooks
    │   ├─ Some(Allow) → ReviewDecision::Approved
    │   ├─ Some(Deny)  → ToolError::Rejected
    │   └─ None → 继续向下
    │
    ├─ guardian_review_id 存在 →
    │   └─ review_approval_request() (Guardian 自动审批)
    │
    └─ 无 guardian →
        └─ with_cached_approval() + session.request_command_approval()
           (用户交互审批，结果缓存)
```

### 审批缓存

`ApprovalStore`（`sandboxing.rs:42`）以 `(command, cwd, sandbox_permissions, additional_permissions)` 为缓存 key：

```rust
// sandboxing.rs:72
async fn with_cached_approval<K, F, Fut>(
    services: &SessionServices,
    tool_name: &str,
    keys: Vec<K>,
    fetch: F,
) -> ReviewDecision
```

- 所有 key 都已缓存为 `ApprovedForSession` → 跳过审批
- 新的 `ApprovedForSession` 决策 → 逐 key 缓存，后续子集也可自动通过

## 执行层：ShellRuntime

`runtimes/shell.rs` — 实现 `Approvable`、`Sandboxable`、`ToolRuntime` 三个 trait。

### ShellRequest 结构

```rust
// runtimes/shell.rs:50
pub struct ShellRequest {
    pub command: Vec<String>,
    pub hook_command: String,
    pub cwd: AbsolutePathBuf,
    pub timeout_ms: Option<u64>,
    pub env: HashMap<String, String>,
    pub explicit_env_overrides: HashMap<String, String>,
    pub network: Option<NetworkProxy>,
    pub sandbox_permissions: SandboxPermissions,
    pub additional_permissions: Option<AdditionalPermissionProfile>,
    pub justification: Option<String>,
    pub exec_approval_requirement: ExecApprovalRequirement,
}
```

### ToolRuntime::run() 执行逻辑

`runtimes/shell.rs:243` — 实际执行命令的核心方法：

```rust
async fn run(
    &mut self,
    req: &ShellRequest,
    attempt: &SandboxAttempt<'_>,
    ctx: &ToolCtx,
) -> Result<ExecToolCallOutput, ToolError> {
    let session_shell = ctx.session.user_shell();
    let managed_network = managed_network_for_sandbox_permissions(
        req.network.as_ref(), req.sandbox_permissions,
    );
    let env = exec_env_for_sandbox_permissions(&req.env, req.sandbox_permissions);

    // 命令包装：locale / snapshot 等
    let command = maybe_wrap_shell_lc_with_snapshot(&req.command, ...);
    // PowerShell 添加 UTF-8 前缀
    let command = if matches!(session_shell.shell_type, ShellType::PowerShell) {
        prefix_powershell_script_with_utf8(&command)
    } else { command };

    // ZshFork 后端路径
    if self.backend == ShellRuntimeBackend::ShellCommandZshFork {
        if let Some(out) = zsh_fork_backend::maybe_run_shell_command(...).await? {
            return Ok(out);
        }
    }

    // 常规执行路径
    let command = build_sandbox_command(&command, &req.cwd, &env, ...)?;
    let options = ExecOptions { expiration, capture_policy };
    let env = attempt.env_for(command, options, managed_network)?;
    let out = execute_env(env, Self::stdout_stream(ctx)).await?;
    Ok(out)
}
```

### 后端类型

| 后端 | 说明 | 使用场景 |
|------|------|---------|
| `Generic` | 默认执行路径 | `shell`、`local_shell`、`container.exec` |
| `ShellCommandClassic` | 标准 shell 路径 | `shell_command` 工具 |
| `ShellCommandZshFork` | zsh fork + shell-escalation | Unix 下 `shell_command` 工具，失败回退到 Classic |

## 沙箱系统

### SandboxAttempt

`sandboxing.rs:374` — 沙箱执行的完整上下文：

```rust
pub(crate) struct SandboxAttempt<'a> {
    pub sandbox: SandboxType,                    // None / Landlock / Windows Sandbox
    pub permissions: &'a PermissionProfile,      // 权限配置
    pub enforce_managed_network: bool,           // 是否强制网络代理
    pub manager: &'a SandboxManager,             // 沙箱管理器
    pub sandbox_cwd: &'a AbsolutePathBuf,        // 沙箱工作目录
    pub codex_linux_sandbox_exe: Option<&'a PathBuf>,  // Linux 沙箱可执行文件
    pub use_legacy_landlock: bool,               // 是否使用旧版 Landlock
    pub windows_sandbox_level: WindowsSandboxLevel,     // Windows 沙箱级别
    pub windows_sandbox_private_desktop: bool,          // Windows 私有桌面
    pub network_denial_cancellation_token: Option<CancellationToken>,  // 网络拒绝取消令牌
}
```

### 沙箱变换

`SandboxAttempt::env_for()` 调用 `SandboxManager::transform()` 将命令变换为沙箱内可执行的形式：

```rust
// sandboxing.rs:388
pub fn env_for(
    &self,
    command: SandboxCommand,
    options: ExecOptions,
    network: Option<&NetworkProxy>,
) -> Result<ExecRequest, SandboxTransformError> {
    self.manager.transform(SandboxTransformRequest {
        command,
        permissions: self.permissions,
        sandbox: self.sandbox,
        enforce_managed_network: self.enforce_managed_network,
        network,
        sandbox_policy_cwd: self.sandbox_cwd,
        // ...
    })
}
```

## 安全机制汇总

| 层级 | 机制 | 代码位置 |
|------|------|---------|
| 命令安全 | `is_known_safe_command()` | 各 Handler 的 `is_mutating()` |
| 执行策略 | `ExecApprovalRequirement` | `shell.rs:228` |
| 审批缓存 | `ApprovalStore` + `with_cached_approval()` | `sandboxing.rs:42` |
| Hook 拦截 | `run_permission_request_hooks()` | `orchestrator.rs:403` |
| Guardian | 自动审批系统 | `orchestrator.rs:190` |
| 沙箱隔离 | `SandboxManager` (Landlock / Windows) | `SandboxAttempt::env_for()` |
| 网络控制 | `NetworkProxy` + managed network | `managed_network_for_sandbox_permissions()` |
| 失败重试 | 沙箱拒绝 → 无沙箱重试 | `orchestrator.rs:264-378` |
| 超时控制 | `ExecExpiration` + `CancellationToken` | `exec.rs` |

### 审批策略矩阵

| `AskForApproval` 策略 | 受限沙箱 | 无沙箱重试 | 说明 |
|----------------------|---------|-----------|------|
| `Never` | 不审批 | 不重试 | 完全自动，信任所有命令 |
| `OnFailure` | 不审批 | 可重试 | 首次沙箱执行，失败后询问 |
| `OnRequest` | 需审批 | 可重试 | 每次受限操作都审批 |
| `Granular` | 需审批 | 可重试 | 细粒度权限控制 |
| `UnlessTrusted` | 需审批 | 可重试 | 默认审批，除非信任配置 |

## 代码示例

### 添加自定义 Handler

```rust
use crate::tools::registry::{ToolHandler, ToolKind};
use crate::tools::context::{ToolInvocation, FunctionToolOutput, ToolPayload};
use crate::function_tool::FunctionCallError;

pub struct MyShellHandler;

#[async_trait::async_trait]
impl ToolHandler for MyShellHandler {
    type Output = FunctionToolOutput;

    fn tool_name(&self) -> codex_tools::ToolName {
        codex_tools::ToolName::plain("my_shell")
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let ToolInvocation { session, turn, tracker, call_id, payload, .. } = invocation;
        let ToolPayload::Function { arguments } = payload else {
            return Err(FunctionCallError::RespondToModel("unsupported payload".into()));
        };

        let params: ShellToolCallParams = parse_arguments_with_base_path(&arguments, &turn.cwd)?;
        let exec_params = ShellHandler::to_exec_params(&params, turn.as_ref(), session.conversation_id);

        run_exec_like(RunExecLikeArgs {
            tool_name: ToolName::plain("my_shell"),
            exec_params,
            hook_command: shlex_join(&params.command),
            additional_permissions: None,
            prefix_rule: None,
            session, turn, tracker, call_id,
            freeform: false,
            shell_runtime_backend: ShellRuntimeBackend::Generic,
        }).await
    }
}
```

### 审批策略配置

```toml
# config.toml
[permissions]
# 审批策略：Never | OnFailure | OnRequest | Granular | UnlessTrusted
ask_for_approval = "OnRequest"

# 文件系统沙箱策略
[permissions.file_system_sandbox]
kind = "Restricted"  # Restricted | Unrestricted

# 网络沙箱策略
[permissions.network_sandbox]
kind = "Managed"     # Managed | Unrestricted | Disabled
```

## 注意事项

- **沙箱兼容性**：Landlock 仅在 Linux 5.13+ 可用，Windows Sandbox 仅在 Windows Pro/Enterprise 可用
- **环境变量注入**：`dependency_env` 和 `explicit_env_overrides` 会按优先级合并到执行环境中
- **ZshFork 回退**：当 zsh fork 后端条件不满足时，自动回退到 Classic 后端
- **网络审批**：Immediate 模式在执行前完成审批，Deferred 模式在执行后异步审批
- **Guardian 审批**：strict_auto_review 模式下，即使 Skip 策略也会走 Guardian 审批

---

## 相关概念

- [工具系统](./tool-system.md) — 核心工具抽象和执行机制
- [Shell 后台执行与超时](./shell-background-timeout.md) — 超时后保持后台运行
- [MCP 协议](./mcp.md) — 第三方服务集成标准

---

**下一页**: [Shell 后台执行与超时](./shell-background-timeout.md) | [工具系统](./tool-system.md)
