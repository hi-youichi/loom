---
sidebar_position: 3
title: "Codex CLI 参考手册"
description: "Codex CLI 的完整命令行选项、沙盒策略、配置类型和事件协议参考"
---

# Codex CLI 参考手册

本文档是 Codex CLI 的完整参考，涵盖命令行选项、沙盒策略、配置体系和事件协议。

> **源码位置**：`thirdparty/codex/codex-rs/`

## 概述

Codex 是一个基于 Rust 的 AI Agent 执行框架，提供：

- **`codex exec`**：非交互式执行引擎，用于自动化和 CI/CD
- **`codex` (TUI)**：全屏终端交互界面
- **JSONL 流式输出**：机器可读的事件流
- **沙盒执行**：可配置的安全策略
- **MCP 协议支持**：外部工具集成

## Crate 架构

```
codex-rs/
├── exec/           # 非交互式执行引擎 (codex exec)
├── tui/            # 全屏终端 UI
├── cli/            # CLI 入口和多工具子命令
├── core/           # 核心业务逻辑、工具系统、编排器
├── protocol/       # 协议类型：SandboxPolicy、SessionId 等
├── config/         # 配置加载与合并
├── sandboxing/     # 沙盒管理器实现
├── rmcp-client/    # MCP 客户端
├── rollout-trace/  # 执行追踪与回放
├── app-server-*/   # 应用层服务器和传输
└── utils/cli/      # 共享 CLI 选项
```

## 命令行选项

### `codex exec`

非交互式执行模式，用于自动化脚本和 CI/CD。

```bash
codex exec [OPTIONS] [PROMPT]
```

#### 基础选项

| 选项 | 短选项 | 类型 | 默认值 | 说明 |
|------|--------|------|--------|------|
| `PROMPT` | — | 位置参数 | — | 初始指令。省略或 `-` 则从 stdin 读取 |
| `--model` | `-m` | `string` | 配置文件 | 使用的模型 |
| `--sandbox` | `-s` | `enum` | `read-only` | 沙盒策略 |
| `--cd` | `-C` | `DIR` | 当前目录 | 工作根目录 |
| `--image` | `-i` | `FILE` | — | 附加图片（可多次指定） |
| `--add-dir` | — | `DIR` | — | 额外可写目录（可多次指定） |

#### 输出控制

| 选项 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `--json` | `bool` | `false` | 启用 JSONL 流式输出（别名 `--experimental-json`） |
| `--output-last-message` | `-o FILE` | — | 将最后一条 `agent_message` 写入文件 |
| `--output-schema` | `FILE` | — | 指定模型最终响应的 JSON Schema |
| `--color` | `enum` | `auto` | 颜色输出：`always`、`never`、`auto` |

#### 会话与配置

| 选项 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `--ephemeral` | `bool` | `false` | 不持久化会话文件 |
| `--ignore-user-config` | `bool` | `false` | 不加载用户 `config.toml` |
| `--ignore-rules` | `bool` | `false` | 不加载 `.rules` 文件 |
| `--profile` | `-p` | `string` | — | 配置 profile 名称 |
| `--skip-git-repo-check` | `bool` | `false` | 允许在 Git 仓库外运行 |

#### 开源模型

| 选项 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `--oss` | `bool` | `false` | 使用开源模型提供商 |
| `--local-provider` | `string` | — | 本地提供商：`lmstudio` 或 `ollama` |

#### 安全

| 选项 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `--sandbox` | `-s` | `enum` | `read-only` | 沙盒策略（见下方） |
| `--dangerously-bypass-approvals-and-sandbox` | `bool` | `false` | 跳过所有确认和沙盒（别名 `--yolo`） |

### 子命令

#### `codex exec resume`

恢复之前的会话。

```bash
codex exec resume [SESSION_ID] [OPTIONS]
codex exec resume --last [PROMPT]
```

| 选项 | 类型 | 说明 |
|------|------|------|
| `SESSION_ID` | 位置参数 | 会话 UUID 或线程名称 |
| `--last` | `bool` | 恢复最近的会话 |
| `--all` | `bool` | 显示所有会话（不按 cwd 过滤） |
| `--image` | `-i FILE` | 附加图片 |
| `PROMPT` | 位置参数 | 恢复后发送的提示 |

#### `codex exec review`

对代码仓库进行审查。

```bash
codex exec review [OPTIONS] [PROMPT]
```

| 选项 | 类型 | 说明 |
|------|------|------|
| `--uncommitted` | `bool` | 审查未提交的变更（staged + unstaged + untracked） |
| `--base BRANCH` | `string` | 对比指定分支的变更 |
| `--commit SHA` | `string` | 审查指定 commit 引入的变更 |
| `--title TITLE` | `string` | Commit 标题（配合 `--commit`） |
| `PROMPT` | 位置参数 | 自定义审查指令 |

## 沙盒策略

### SandboxMode（配置级别）

来源：`protocol/src/config_types.rs`

```rust
enum SandboxMode {
    ReadOnly,           // 默认：只读文件系统
    WorkspaceWrite,     // 允许写入工作目录
    DangerFullAccess,   // 无限制
}
```

### SandboxPolicy（运行时策略）

来源：`protocol/src/protocol.rs`

```rust
enum SandboxPolicy {
    /// 无任何限制，谨慎使用
    DangerFullAccess,

    /// 只读访问
    ReadOnly {
        network_access: bool,   // 默认 false
    },

    /// 外部沙盒环境，允许完整磁盘访问但遵守网络设置
    ExternalSandbox {
        network_access: NetworkAccess,  // Enabled | Disabled
    },

    /// 工作区可写（等同于 ReadOnly + 当前目录写入权限）
    WorkspaceWrite {
        writable_roots: Vec<PathBuf>,   // 额外可写目录
        network_access: bool,           // 默认 false
        exclude_tmpdir_env_var: bool,   // 不包含用户 TMPDIR
        exclude_slash_tmp: bool,        // 不包含 /tmp
    },
}
```

### 策略对比

| 能力 | `ReadOnly` | `WorkspaceWrite` | `DangerFullAccess` | `ExternalSandbox` |
|------|-----------|-----------------|-------------------|------------------|
| 读取文件系统 | ✅ | ✅ | ✅ | ✅ |
| 写入工作目录 | ❌ | ✅ | ✅ | ✅ |
| 写入额外目录 | ❌ | 通过 `writable_roots` | ✅ | ✅ |
| 网络访问 | 可配置 | 可配置 | ✅ | 可配置 |
| 受保护路径 | — | `.git`、`.codex` 只读 | — | — |
| 使用场景 | 审查/分析 | 代码生成/修改 | 外部沙盒 | CI/容器 |

### CLI 沙盒映射

```bash
--sandbox read-only           → SandboxPolicy::ReadOnly { network_access: false }
--sandbox workspace-write     → SandboxPolicy::WorkspaceWrite { writable_roots: [], ... }
--sandbox danger-full-access  → SandboxPolicy::DangerFullAccess
```

## 审批系统

### ApprovalsReviewer

来源：`protocol/src/config_types.rs`

```rust
enum ApprovalsReviewer {
    User,        // 默认：路由给用户确认
    AutoReview,  // 自动审批子代理（风险决策框架）
}
```

审批请求的触发场景：

- 沙盒逃逸尝试
- 阻止的网络访问
- MCP 审批提示
- ARC（Agent Request for Confirmation）升级

## Shell 环境策略

来源：`protocol/src/config_types.rs`

```rust
struct ShellEnvironmentPolicy {
    inherit: ShellEnvironmentPolicyInherit,  // Core | All（默认）| None
    ignore_default_excludes: bool,           // 默认 true
    exclude: Vec<Pattern>,                   // 排除的变量名模式
    set: HashMap<String, String>,            // 注入的环境变量
    include_only: Vec<Pattern>,              // 仅保留的变量名模式
    use_profile: bool,                       // 是否使用 shell profile
}
```

环境构建流程：

1. 根据 `inherit` 策略创建初始环境
2. 若 `ignore_default_excludes` 为 false，过滤含 `KEY`、`SECRET`、`TOKEN` 的变量
3. 应用 `exclude` 模式过滤
4. 注入 `set` 中的条目
5. 若 `include_only` 非空，仅保留匹配的变量

## 配置体系

### 配置层级

```
1. CLI 参数（最高优先级）
2. --profile 指定的配置 profile
3. 项目 .codex/config.toml
4. 用户 ~/.codex/config.toml
5. 默认值
```

### 关键配置项

```toml
# ~/.codex/config.toml

[model]
name = "o3"

[sandbox]
mode = "workspace-write"          # read-only | workspace-write | danger-full-access

[approvals]
reviewer = "user"                 # user | auto_review

[shell_env]
inherit = "all"                   # core | all | none
ignore_default_excludes = true
exclude = ["*SECRET*"]
use_profile = false

[features]
goals = true                      # 启用 /goal Ralph Loop 模式
```

## 事件协议

> 完整字段级参考见 [Codex 事件协议字段参考](./codex-event-protocol.md)，本节仅提供概览。

### 事件流生命周期

```
thread.started
  └── turn.started
        ├── item.started
        │     ├── item.updated (0..N)
        │     └── item.completed
        ├── item.started ...
        └── turn.completed / turn.failed
  └── turn.started ...
error (不可恢复错误，任意时刻)
```

### 核心事件类型

来源：`exec/src/exec_events.rs`

| 事件 `type` | Rust 类型 | 字段 |
|-------------|-----------|------|
| `thread.started` | `ThreadStartedEvent` | `thread_id` |
| `turn.started` | `TurnStartedEvent` | — |
| `turn.completed` | `TurnCompletedEvent` | `usage: Usage` |
| `turn.failed` | `TurnFailedEvent` | `error: ThreadErrorEvent` |
| `item.started` | `ItemStartedEvent` | `item: ThreadItem` |
| `item.updated` | `ItemUpdatedEvent` | `item: ThreadItem` |
| `item.completed` | `ItemCompletedEvent` | `item: ThreadItem` |
| `error` | `ThreadErrorEvent` | `message` |

### Usage 统计

```rust
struct Usage {
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
}
```

### ThreadItem 类型

| `type` 值 | Rust 类型 | 关键字段 | 状态枚举 |
|-----------|-----------|----------|----------|
| `agent_message` | `AgentMessageItem` | `text` | — |
| `reasoning` | `ReasoningItem` | `text` | — |
| `command_execution` | `CommandExecutionItem` | `command`、`aggregated_output`、`exit_code` | `InProgress` `Completed` `Failed` `Declined` |
| `file_change` | `FileChangeItem` | `changes: [{path, kind}]` | `InProgress` `Completed` `Failed` |
| `mcp_tool_call` | `McpToolCallItem` | `server`、`tool`、`arguments`、`result`、`error` | `InProgress` `Completed` `Failed` |
| `collab_tool_call` | `CollabToolCallItem` | `tool`、`sender_thread_id`、`receiver_thread_ids`、`agents_states` | `InProgress` `Completed` `Failed` |
| `web_search` | `WebSearchItem` | `id`、`query`、`action` | — |
| `todo_list` | `TodoListItem` | `items: [{text, completed}]` | — |
| `error` | `ErrorItem` | `message` | — |

### 文件变更类型

```rust
enum PatchChangeKind {
    Add,      // 新建文件
    Delete,   // 删除文件
    Update,   // 修改文件
}
```

### 协作工具类型

```rust
enum CollabTool {
    SpawnAgent,   // 启动子代理
    SendInput,    // 发送输入
    Wait,         // 等待子代理完成
    CloseAgent,   // 关闭子代理
}

enum CollabAgentStatus {
    PendingInit,
    Running,
    Interrupted,
    Completed,
    Errored,
    Shutdown,
    NotFound,
}
```

## 其他配置类型

### ReasoningSummary

```rust
enum ReasoningSummary {
    Auto,       // 默认
    Concise,
    Detailed,
    None,       // 禁用推理摘要
}
```

### Verbosity

```rust
enum Verbosity {
    Low,
    Medium,     // 默认
    High,
}
```

## 相关文档

- [CLI JSON 流式输出](../deployment/cli-json-output.md) — JSONL 事件流的详细使用指南
- [Claude Code JSON 协议参考](./claude-code-json-protocol.md) — Claude Code CLI 的 JSON 输出协议
- [Claude Code JSON Schema 类型详解](./claude-code-schema-types.md) — Schema 核心类型
- [Claude Code 兼容层设计](../design/claude-code-compat.md) — Loom 与 Claude Code 协议的适配架构
- [Codex /goal 功能](../design/codex-goal-feature.md) — Ralph Loop 模式详解
