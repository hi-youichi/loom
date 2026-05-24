---
sidebar_position: 8
title: "Goal 外部循环 — 开发任务清单"
description: "基于 Codex /goal 源码对齐的逐步开发任务清单，Agent 通过 MCP task 工具参与完成判定"
---

# Goal 外部循环 — 开发任务清单

> 基于 [开发方案](./goal-external-loop-dev-plan.md)，按依赖顺序拆解为可独立验证的最小任务单元。
> 每个任务完成后立即 `cargo check/test`，不带着编译错误进入下一个任务。

---

## Phase 1 — task-core sqlx 迁移 + 核心类型

### 1.1 `task-core/Cargo.toml` — 依赖切换

- [x] 移除 `rusqlite` 依赖
- [x] 添加 `sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }`
- [x] 添加 `thiserror`
- [x] 运行 `cargo check -p task-core`（预期失败）

### 1.2 `task-core/src/models.rs` — FromRow derive

- [x] 为 `Task` struct 添加 `#[derive(sqlx::FromRow)]`
- [x] `TaskStatus` 实现 `sqlx::Type` + `sqlx::Decode`（或 TEXT 列 + 手动解析）
- [x] 运行 `cargo check -p task-core`

### 1.3 `task-core/migrations/` — 迁移文件

- [x] 创建 `task-core/migrations/` 目录
- [x] 将现有 `INIT_SQL` 拆为 `20250101000000_initial.sql`
- [x] 创建 `20250102000000_goal_fields.sql`：`ALTER TABLE tasks ADD COLUMN metadata TEXT NOT NULL DEFAULT '{}'`
- [x] 运行 `cargo check -p task-core`

### 1.4 `task-core/src/db.rs` — sqlx 重写

- [x] `TaskDb` 内部从 `Mutex<Connection>` 改为 `SqlitePool`
- [x] `open()` → `async fn open()` 使用 `SqlitePoolOptions::new().connect()` + `sqlx::migrate!()`
- [x] `create_task()` → `async fn create_task()` 使用 `sqlx::query_as()`
- [x] `show_task()` → `async fn show_task()`
- [x] `list_tasks()` → `async fn list_tasks()`
- [x] `update_task()` → `async fn update_task()`
- [x] `delete_task()` → `async fn delete_task()`
- [x] `find_by_id_prefix()` → `async fn find_by_id_prefix()`
- [x] 统一错误类型：`thiserror` 定义 `TaskDbError`
- [x] 编写单元测试：现有 CRUD 操作全部通过
- [x] 编写迁移测试：旧数据库打开后自动迁移
- [x] 运行 `cargo test -p task-core`

### 1.5 `task-core/src/db.rs` — Goal 新增 API

- [x] 实现 `async fn get_meta(task_id, key)`
- [x] 实现 `async fn set_meta(task_id, key, value)`
- [x] 实现 `async fn atomic_update_status(id, from, to)`
- [x] 编写单元测试：get_meta / set_meta 读写正确
- [x] 编写单元测试：atomic_update_status 原子性
- [x] 运行 `cargo test -p task-core`

### 1.6 消费者适配 — loom tools

- [x] `loom/src/tools/task/create.rs` — `db.method()` → `db.method().await`
- [x] `loom/src/tools/task/show.rs` — 同上
- [x] `loom/src/tools/task/list.rs` — 同上
- [x] `loom/src/tools/task/update.rs` — 同上
- [x] `loom/src/tools/task/delete.rs` — 同上
- [x] `loom/src/agent/react/build/tool_source.rs` — `TaskDb::open()` 加 `.await`
- [x] 运行 `cargo check -p loom`

### 1.7 消费者适配 — task-cli / task-mcp-server

- [x] `task-cli/src/main.rs` — 添加 `#[tokio::main]`，所有 db 调用加 `.await`
- [x] `task-mcp-server/src/main.rs` — 同上
- [x] 运行 `cargo check -p task-cli`
- [x] 运行 `cargo check -p task-mcp-server`

### 1.8 `loom/src/goal_runner/` — 模块骨架

- [x] 创建 `loom/src/goal_runner/mod.rs`
- [x] 在 `loom/src/lib.rs` 中添加 `pub mod goal_runner`
- [x] 创建空文件：`state.rs`, `tool.rs`, `runner.rs`, `message.rs`
- [x] 运行 `cargo check -p loom`

### 1.9 `loom/src/goal_runner/state.rs` — 核心类型定义

- [x] 定义 `TurnResult` struct（`reply`, `reasoning_content`, `tool_calls_summary`, `usage`）
- [x] 定义 `ToolCallSummary` struct（`tool_name`, `result_preview`）
- [x] 定义 `ToolError` enum（`ExecutionFailed`, `Timeout`, `Aborted`）
- [x] 定义 `GoalOutcome` enum（`Achieved`, `Error(String)`）
- [x] 在 `mod.rs` 中 re-export 所有类型
- [x] 运行 `cargo check -p loom`

**Phase 1 完成标准**：`cargo check --workspace && cargo test -p task-core` 全部通过

---

## Phase 2 — CodingTool trait + LoomTool + MCP 集成

### 2.1 `loom/src/goal_runner/tool.rs` — CodingTool trait

- [x] 定义 `CodingTool` trait（`async fn execute() -> Result<TurnResult, ToolError>` + `fn name() -> &str`）
- [x] 添加 `#[async_trait]` 属性
- [x] 定义 `ShellTool` struct 骨架
- [x] 运行 `cargo check -p loom`

### 2.2 `loom/src/goal_runner/tool.rs` — LoomTool + MCP 连接

- [x] 定义 `LoomTool` struct（`session_id`, `task_id`, `db_path`, `mcp_stdin`, `mcp_stdout`）
- [x] LoomTool 接收 GoalRunner 已启动的 MCP server 的 stdin/stdout handles
- [x] LoomTool 的 `execute()` 中通过 MCP 连接调用 `task_show` / `task_update`
- [x] `execute()` 中从 `ReActState` 提取 `TurnResult`
- [x] 运行 `cargo check -p loom`

### 2.3 MCP server 子进程生命周期

- [x] GoalRunner::new() 中启动 task-mcp-server 子进程
- [x] GoalRunner::cleanup() 中关闭子进程
- [x] 编写单元测试：子进程启动/关闭
- [x] 运行 `cargo test -p loom`

**Phase 2 完成标准**：LoomTool 编译通过 + MCP server 子进程正确启动/关闭

---

## Phase 3 — GoalRunner 基础循环

### 3.1 `loom/src/goal_runner/runner.rs` — GoalRunner struct

- [x] 定义 `GoalRunner` struct（`task_id`, `objective`, `db: Arc<TaskDb>`（直接操作）, `tool`, `mcp_server: Child`（仅供 Agent）, `working_dir`, `iteration`, `max_iterations=100`, `cancel`, `consecutive_failures`, `time_used_seconds`）
- [x] 实现 `new()` 构造函数（通过 db 直接创建 Task + 启动 MCP server 子进程）
- [x] 实现 `cleanup()` 关闭 MCP server 子进程
- [x] 运行 `cargo check -p loom`

### 3.2 `loom/src/goal_runner/runner.rs` — save_iteration_state()

- [x] 实现 `save_iteration_state()`：追加到 `metadata["goal"].history[]`
- [x] 保留最近 20 条
- [x] 记录 iteration / time_used_seconds / timestamp
- [x] 编写单元测试
- [x] 运行 `cargo test -p loom`

### 3.3 `loom/src/goal_runner/runner.rs` — run() 主循环

- [x] 实现 `run() -> GoalOutcome`
- [x] 循环：continuation → execute → save → check task.status
- [x] max_iterations=100 检查（循环顶部）
- [x] ToolError 处理：Aborted → save Paused + cleanup + Error, Timeout → continue, Failed → consecutive_failures
- [x] 连续 3 次失败 → cleanup + Error
- [x] 每轮检查 task.status == Completed → cleanup + Achieved
- [x] 编写集成测试：Agent 通过 MCP task_update 标记完成 → Achieved
- [x] 编写集成测试：max_iterations 耗尽 → Error
- [x] 编写单元测试：连续 3 次失败 → Error
- [x] 编写测试：MCP server 在完成时正确关闭
- [x] 运行 `cargo test -p loom`

**Phase 3 完成标准**：run() 集成测试全绿 + MCP server 生命周期正确

---

## Phase 4 — Continuation Prompt

### 4.1 `loom/src/goal_runner/message.rs` — continuation 模板

- [x] 实现 `build_continuation_prompt()` — Codex continuation.md 英文原文
- [x] 包含 `<untrusted_objective>` 标签 + escape_xml_text
- [x] 包含 time_used_seconds
- [x] 包含完整 completion audit 7 条检查项（与 Codex 对齐）
- [x] 工具名替换：`update_goal` → `task_update`, `get_goal` → `task_show`
- [x] 注入为 **developer 消息**角色（与 Codex 对齐）
- [x] 运行 `cargo check -p loom`

### 4.2 `loom/src/goal_runner/message.rs` — escape_xml_text

- [x] 实现 `escape_xml_text()`：转义 `& < >`
- [x] 编写单元测试：正确转义（与 Codex 测试对齐）
- [x] 运行 `cargo test -p loom`

**Phase 4 完成标准**：continuation prompt 与 Codex 模板一致（仅工具名不同）

---

## Phase 5 — CLI 集成

### 5.1 `cli/src/args.rs` — GoalArgs 定义

- [x] 定义 `GoalArgs` struct（description, tool, resume, id, verbose）
- [x] 在 `Command` enum 中添加 `Goal(GoalArgs)` 变体
- [x] 运行 `cargo check -p cli`

### 5.2 `cli/src/goal_cmd.rs` — 命令入口

- [x] 实现 `run_goal(args: GoalArgs) -> Result<()>`
- [x] 构造 TaskDb + CancellationToken
- [x] 调用 `GoalRunner::new()`（内部创建 Task + 启动 MCP server + 构造 LoomTool）
- [x] 调用 `run()` 并输出 GoalOutcome
- [x] 确保 MCP server 在完成/错误时正确关闭（GoalRunner::cleanup）
- [x] 运行 `cargo check -p cli`

### 5.3 端到端验证

- [x] `cargo build -p cli`
- [x] `loom goal "echo hello"` 端到端成功
- [x] task 记录正确创建
- [x] MCP server 进程正确退出

**Phase 5 完成标准**：`loom goal "echo hello"` 端到端运行成功

---

## Phase 6 — 外部工具

### 6.1 `loom/src/goal_runner/tool.rs` — ShellTool 实现

- [x] 实现 `ShellTool::execute()`：执行 shell 命令，构造 TurnResult
- [x] 超时处理：`tokio::time::timeout`
- [x] exit_code 非零 → ToolError::ExecutionFailed
- [x] 运行 `cargo check -p loom`

### 6.2 外部工具 MCP 配置

- [x] CodexTool：生成 codex CLI 的 MCP config（指向 task-mcp-server）
- [x] ClaudeTool：生成 claude CLI 的 MCP config
- [x] CursorTool：生成 cursor CLI 的 MCP config
- [x] 运行 `cargo check -p loom`

### 6.3 工具记忆

- [x] GoalRunner 启动时写入 `metadata["goal"].tool`
- [x] 恢复时读取并构造对应 CodingTool
- [x] 编写单元测试
- [x] 运行 `cargo test -p loom`

**Phase 6 完成标准**：ShellTool 实现完整 + 外部工具 MCP 配置 + 工具记忆

---

## Phase 7 — 持久化与恢复

### 7.1 Ctrl+C 信号处理

- [x] 在 `goal_cmd.rs` 中注册 `ctrlc::set_handler` → CancellationToken::cancel()
- [x] GoalRunner 在 execute 前检查 cancel
- [x] 被取消时：save iteration/tool/time_used 到 metadata，task.status = Pending，关闭 MCP server
- [x] 运行 `cargo check -p cli`

### 7.2 恢复流程

- [x] 实现 `GoalRunner::resume(id, db)`
- [x] 使用 `atomic_update_status(id, Pending, InProgress)`
- [x] 从 metadata 恢复 iteration / tool / time_used_seconds
- [x] 重新启动 task-mcp-server 子进程
- [x] 编写单元测试：正常恢复
- [x] 编写单元测试：并发恢复被拒绝
- [x] 运行 `cargo test -p loom`

### 7.3 CLI 集成

- [x] `loom goal --resume --id <task_id>` 路由到 resume
- [x] 手动测试：启动 → Ctrl+C → resume → 继续
- [x] 运行 `cargo test -p cli`

**Phase 7 完成标准**：中断后可成功恢复并继续循环（MCP server 重新启动）

---

## Phase 8 — 可观测性

### 8.1 tracing 日志

- [x] 每轮迭代输出 tracing 日志：iteration / time_used_seconds
- [x] 日志中包含 `session_id = task_id`
- [x] 运行 `cargo check -p loom`

### 8.2 history 追加

- [x] `save_iteration_state()` 追加记录（iteration / time_used_seconds / timestamp）
- [x] 默认保留最近 20 条
- [x] 编写单元测试：history 长度限制
- [x] 运行 `cargo test -p loom`

### 8.3 stderr 进度摘要

- [x] `--verbose` 参数启用 stderr 进度输出
- [x] 每轮输出：`[iteration N] time: 120s`
- [x] 运行 `cargo check -p cli`

**Phase 8 完成标准**：可从 task metadata 查看完整迭代历史

---

## 全局验收清单

- [x] `cargo check --workspace` 无警告
- [x] `cargo test --workspace` 全绿
- [x] `cargo clippy --workspace` 无警告
- [x] 端到端：`loom goal "在 /tmp 下创建 hello.txt 并写入 hello world"` 成功
- [x] 外部工具：`loom goal --tool codex "echo hello"` 成功
- [x] MCP：Agent 通过 MCP 调用 task_update 标记完成
- [x] 恢复：Ctrl+C → `loom goal --resume --id <id>` 恢复成功
- [x] 可观测性：`loom goal --verbose "简单任务"` stderr 输出进度
