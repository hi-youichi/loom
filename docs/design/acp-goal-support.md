# ACP 支持 Goal 的方案（已实现）

> 在 ACP 协议中通过 `/goal` 命令触发自主目标循环，复用现有 GoalRunner。

## 实现概述

通过 `/goal` 命令 + 复用 `loom::goal_runner::GoalRunner` 实现：

1. 用户在 IDE 输入 `/goal <描述>`
2. ACP `prompt()` 检测命令 → 委托给 `crate::goal_runner::run_goal()`
3. `run_goal()` 创建 `TaskDb`、`LoomTool`（带事件桥接）和 `GoalRunner`
4. 在单次 `prompt()` 调用内运行整个目标循环
5. 通过 `session/update` 实时推送迭代进度和工具调用
6. Agent 通过 MCP 注册的 task 工具操作 task 状态

## 已修改/新增的文件

### 新增文件

| 文件 | 说明 |
|------|------|
| `loom-acp/src/goal_runner.rs` | ACP 目标运行器封装，创建 GoalRunner 并运行 |

### 修改文件

| 文件 | 变更 |
|------|------|
| `loom/src/command/command.rs` | `Command` 枚举新增 `Goal { description: String }` 变体 |
| `loom/src/command/parser.rs` | 解析 `/goal <描述>` 为 `Command::Goal`；新增 2 个测试 |
| `loom/src/command/builtins.rs` | `Goal` 命令走 `PassThrough` 路径 |
| `loom-acp/Cargo.toml` | 新增 `task-core` 依赖 |
| `loom-acp/src/lib.rs` | 注册 `pub mod goal_runner` |
| `loom-acp/src/agent.rs` | `prompt()` 中检测 `/goal` 并调用 `run_goal()` |

## 架构

```
用户在 IDE 输入: /goal 迁移到 Pydantic v2
    │
    ▼
ACP Agent::prompt()
    │
    ├── 解析 /goal → Command::Goal { description }
    ├── 构造 event_sender (桥接到 session/update)
    ├── 调用 crate::goal_runner::run_goal()
    │     ├── 创建 TaskDb (tasks.db)
    │     ├── 写入 MCP config (.loom/goal-mcp.json)
    │     ├── 创建 LoomTool (带 event_sender + cancellation)
    │     ├── 创建 GoalRunner (复用 loom::goal_runner::GoalRunner)
    │     └── 运行 GoalRunner::run() → 自主循环
    │           ├── 构造 continuation prompt
    │           ├── 调用 LoomTool.execute() (内部用 run_agent_with_options)
    │           ├── 事件桥接到 SessionNotifier → session/update
    │           └── 检查 task.status == Completed → 退出
    │
    └── 返回 PromptResponse { stop_reason: EndTurn }
```

### 事件流

```
GoalRunner → LoomTool.execute()
    │
    ├── AnyStreamEvent → event_sender closure
    │     └── SessionNotifier::try_send_event()
    │           └── mpsc::Sender<SessionNotification>
    │                 └── session/update → IDE
    │
    └── 最终返回 PromptResponse
```

## 测试覆盖

| 测试 | 验证内容 |
|------|---------|
| `command::parser::tests::parse_goal_with_description` | `/goal fix the login bug` 正确解析 |
| `command::parser::tests::parse_goal_without_description_returns_none` | `/goal` 无参数返回 None |
| `goal_runner::tests::test_run_goal_creates_task_and_db` | TaskDb 和 MCP config 可正确创建 |
| `goal_runner::tests::test_goal_result_fields` | GoalResult 结构体字段正确 |
| `goal_runner::tests::test_goal_run_error_display` | GoalRunError Display 实现正确 |

全部 50 个 loom-acp lib 测试通过。
