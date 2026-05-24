---
sidebar_position: 7
title: "Goal 外部循环 — 开发方案"
description: "基于 Codex /goal 源码对齐的开发方案，Agent 通过 MCP task 工具参与完成判定"
---

# Goal 外部循环 — 开发方案

## 设计决策摘要

| 决策 | 选择 | 依据 |
|------|------|------|
| 评估策略 | Agent 自评（task_update via MCP） | Codex 原始方案，通过 MCP 统一 |
| Agent 工具 | task-mcp-server（task_show / task_update） | 统一所有编码工具的 task 操作方式 |
| MCP 生命周期 | 每次 goal 启动独立子进程（stdio） | 简单、隔离、与 Codex 一致 |
| 循环控制 | 外部 GoalRunner | Codex GoalRuntime 对齐 |
| Continuation | developer 消息 + Codex 英文模板（工具名替换） | Codex 对齐 |
| 安全防护 | `<untrusted_objective>` + escape_xml_text | Codex 对齐 |
| 持久化 | task-core metadata JSON | Codex SQLite 对齐 |
| 时间追踪 | time_used_seconds | Codex 对齐 |
| Token 记账 | 后续实现 | 后续 |
| Token 预算 | 后续实现 | 后续 |

---

## 文件结构

```
loom/src/goal_runner/
├── mod.rs
├── state.rs
├── tool.rs
├── runner.rs
└── message.rs

cli/src/
├── args.rs
└── goal_cmd.rs

task-core/
├── migrations/
│   ├── 20250101000000_initial.sql
│   └── 20250102000000_goal_fields.sql
└── src/
    ├── db.rs
    └── models.rs

task-mcp-server/        # 已有，无需修改
```

注意：不需要新的 goal_tools.rs。Agent 通过 MCP 使用已有的 task-mcp-server。

---

## Phase 1 — task-core sqlx 迁移 + 核心类型

### 1.1 task-core: rusqlite → sqlx

依赖变更、SqlitePool、sqlx::migrate!、sqlx::FromRow、消费者适配。

### 1.2 task-core: Goal 新增 API

get_meta / set_meta / atomic_update_status。

### 1.3 loom: goal_runner 模块 + state.rs

创建模块骨架，定义 TurnResult / ToolCallSummary / ToolError / GoalOutcome。

### 1.4 验收标准

- [ ] `cargo check --workspace` 通过
- [ ] `cargo test -p task-core` 通过
- [ ] get_meta / set_meta 读写正确
- [ ] atomic_update_status 原子性
- [ ] 旧数据库自动迁移

---

## Phase 2 — CodingTool trait + LoomTool + MCP 集成

### 2.1 CodingTool trait

```rust
#[async_trait]
pub trait CodingTool: Send + Sync {
    async fn execute(&self, prompt: &str, working_dir: &Path) -> Result<TurnResult, ToolError>;
    fn name(&self) -> &str;
}
```

### 2.2 LoomTool + MCP 连接

LoomTool 封装 ReactRunner，接收 GoalRunner 已启动的 MCP server 连接。

```rust
pub struct LoomTool {
    session_id: String,
    task_id: String,
    db_path: PathBuf,
    mcp_stdin: Box<dyn AsyncWrite>,   // GoalRunner 启动的 MCP server stdin
    mcp_stdout: Box<dyn AsyncRead>,   // GoalRunner 启动的 MCP server stdout
}
```

LoomTool 的 `execute()` 中：
1. 通过 mcp_stdin/mcp_stdout 与 task-mcp-server 通信
2. Agent 可通过 MCP 调用 task_show / task_update
3. 从 ReActState 提取 TurnResult

### 2.3 MCP server 子进程生命周期

GoalRunner 负责 MCP server 的生命周期，但 **GoalRunner 自身不走 MCP**：
- GoalRunner 通过 `Arc<TaskDb>` 直接操作 task（create_task / show_task / set_meta）
- MCP server 仅服务于 Agent（task_show / task_update）

GoalRunner 的职责：
- 启动：`Command::new("task-mcp-server").arg("--db-path").spawn()`
- 关闭：Goal 完成或 Ctrl+C 时 `mcp_server.kill()`
- 恢复：resume 时重新启动 MCP server 子进程

### 2.4 ShellTool 骨架

```rust
pub struct ShellTool { command: String, args: Vec<String> }
```

### 2.5 验收标准

- [ ] LoomTool 实现 CodingTool trait
- [ ] MCP server 子进程正确启动和关闭
- [ ] Agent 可通过 MCP 调用 task_show / task_update

---

## Phase 3 — GoalRunner 基础循环

### 3.1 GoalRunner struct

```rust
pub struct GoalRunner {
    task_id: String,
    objective: String,
    db: Arc<TaskDb>,
    tool: Box<dyn CodingTool>,
    mcp_server: Child,
    working_dir: PathBuf,
    iteration: u32,
    max_iterations: u32,
    cancel: CancellationToken,
    consecutive_failures: u32,
    time_used_seconds: i64,
}
```

### 3.2 run() 主循环

```
loop {
    continuation → execute → save_iteration_state → check task.status
}
```

- max_iterations=100（内部安全阀）
- ToolError 处理：Aborted → save Paused, Timeout → continue, Failed → consecutive_failures
- 连续 3 次失败 → Error
- 检查 task.status == Completed（由 Agent 通过 MCP task_update 设置）→ Achieved
- 完成时 cleanup() 关闭 MCP server

### 3.3 save_iteration_state()

追加到 metadata["goal"].history[]，保留最近 20 条。

### 3.4 验收标准

- [ ] 集成测试：Agent 通过 task_update 标记完成 → Achieved
- [ ] 集成测试：max_iterations 耗尽 → Error
- [ ] 连续失败 3 次 → Error
- [ ] MCP server 正确关闭

---

## Phase 4 — Continuation Prompt

### 4.1 build_continuation_prompt()

Codex continuation.md 英文原文模板（含完整 completion audit），工具名替换：
- `update_goal` → `task_update with status='completed'`
- `get_goal` → `task_show`

注入为 **developer 消息**。

### 4.2 escape_xml_text()

```rust
fn escape_xml_text(input: &str) -> String {
    input.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
```

### 4.3 验收标准

- [ ] escape_xml_text 正确转义
- [ ] continuation prompt 包含 `<untrusted_objective>` 标签
- [ ] completion audit 7 条指令完整
- [ ] 工具名正确引用 task_show / task_update
- [ ] 注入为 developer 消息角色

---

## Phase 5 — CLI 集成

### 5.1 GoalArgs

```rust
pub(crate) struct GoalArgs {
    pub(crate) description: String,
    pub(crate) tool: Option<String>,
    pub(crate) resume: bool,
    pub(crate) id: Option<String>,
    pub(crate) verbose: bool,
}
```

### 5.2 goal_cmd.rs

构造 TaskDb + CancellationToken → GoalRunner::new()（内部创建 Task + 启动 MCP server）→ run() → 输出 GoalOutcome。

### 5.3 验收标准

- [ ] `loom goal "echo hello"` 端到端成功
- [ ] task 记录正确创建
- [ ] MCP server 正确启动和关闭

---

## Phase 6 — 外部工具

ShellTool 实现（codex / claude / cursor）。
外部工具通过 MCP config 连接 task-mcp-server。
工具记忆：metadata["goal"].tool。

### 6.1 验收标准

- [ ] ShellTool 正确实现 CodingTool trait
- [ ] CodexTool 通过 MCP config 连接 task-mcp-server
- [ ] 工具记忆读写正确

---

## Phase 7 — 持久化与恢复

Ctrl+C → save Paused → 关闭 MCP server。
loom goal --resume --id `<id>` → 重新启动 MCP server → 恢复状态 → 继续循环。
原子性并发恢复保护。

### 7.1 验收标准

- [ ] Ctrl+C → task.status = Pending + MCP server 关闭
- [ ] resume 恢复 iteration / tool / time_used_seconds + 重新启动 MCP server
- [ ] 并发恢复被拒绝

---

## Phase 8 — 可观测性

tracing 日志 + history 追加（保留最近 20 条）。

### 8.1 验收标准

- [ ] 每轮迭代输出 tracing 日志
- [ ] history 追加正确

---

## Phase 依赖关系

```
Phase 1 (task-core sqlx + 核心类型)
  ↓
Phase 2 (CodingTool + LoomTool + MCP 集成)
  ↓
Phase 3 (GoalRunner 基础循环)
  ↓
Phase 4 (Continuation Prompt)
  ↓
Phase 5 (CLI 集成) ← 端到端可用
  ↓
Phase 6 (外部工具)
  ↓
Phase 7 (持久化与恢复)
  ↓
Phase 8 (可观测性)
```
