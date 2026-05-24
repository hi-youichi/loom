# Goal Runner 子 Agent 中间过程输出方案

## 背景

当前 `GoalRunner` 调用 `LoomTool::execute` 时，`run_agent_with_options` 的 `on_event` 参数传的是 `None`。
这意味着子 agent 执行过程中的所有中间事件（工具调用、流式输出、思考过程等）都被丢弃了，
用户只能看到每次 iteration 结束后的 summary。

**目标**：
1. 把 CLI 的事件输出逻辑抽象到 `loom` crate，使其可复用
2. 让 goal_runner 的子 agent 执行过程能使用这套输出
3. 同时支持其他上层（serve、telegram-bot）接入

## 现有代码分布

### CLI 侧（`cli` crate）

| 文件 | 内容 | 依赖 |
|------|------|------|
| `cli/src/run/display.rs` | 状态格式化函数（`format_react_state_display` 等） | 只依赖 `loom` 类型 |
| `cli/src/run/agent.rs:76-101` | 辅助函数（`log_node_enter`、`log_tools_used`、`print_stream_chunk`、`format_context_limit`） | 只依赖 `loom` 类型 + `eprintln!` |
| `cli/src/run/agent.rs:331-711` | 事件处理器（`on_event_react`、`on_event_dup`、`on_event_tot`、`on_event_got`） | 依赖上述 + `EventState` struct |
| `cli/src/run/agent.rs:590-601` | `EventState` struct | 纯数据 |
| `cli/src/run/agent.rs:271-287` | 组装 `on_event` 闭包并传给 `run_agent_with_options` | 编排层 |

### Loom 侧（`loom` crate）

| 文件 | 内容 |
|------|------|
| `loom/src/cli_run/agent.rs` | `run_agent_with_options`，接收 `on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>>` |
| `loom/src/cli_run/mod.rs` | 导出 `AnyStreamEvent`、`RunOptions` 等 |
| `loom/src/goal_runner/tool.rs` | `LoomTool`，目前 `on_event` 传 `None` |
| `loom/src/goal_runner/runner.rs` | `GoalRunner`，构造 `LoomTool` |

### 关键发现

- `display.rs` 的格式化函数只依赖 `loom` 的类型（`ReActState`、`ToolCall` 等），**不依赖 CLI 状态**，理论上可以直接移到 `loom`
- `on_event_react` 等函数调用 `eprintln!` 直接写终端，需要抽象为**写入 trait**才能复用
- `print_stream_chunk` 区分 Thinking → stderr，其他 → stdout，这个行为需要保留

## 方案

### 总体思路

1. 在 `loom` crate 中新建 `stream_display` 模块，把 CLI 的事件格式化+输出逻辑搬过来
2. 抽象输出目标为 `Write` trait（或自定义 trait），默认实现写 stderr/stdout
3. 提供一个工厂函数，生成可直接传给 `run_agent_with_options` 的 `on_event` 回调
4. `LoomTool` 使用这个工厂函数构造 `on_event`
5. CLI 侧改为调用 `loom` 的工厂函数，删除自己的重复代码

### Phase 1：在 loom 中新建 `stream_display` 模块

#### 1.1 新建文件 `loom/src/stream_display/mod.rs`

```rust
pub mod format;
pub mod event_handler;

pub use event_handler::{create_stdio_event_callback, StreamDisplayConfig};
```

#### 1.2 新建 `loom/src/stream_display/format.rs`

从 `cli/src/run/display.rs` 搬入：
- `truncate_display`
- `format_message_truncated`
- `format_tool_call_truncated`
- `format_tool_result_truncated`
- `format_react_state_display`
- `format_tot_state_display`
- `format_dup_state_display`
- `format_got_state_display`
- `indent_lines`

改为 `pub`（原来是 `pub(crate)`）。

#### 1.3 新建 `loom/src/stream_display/event_handler.rs`

从 `cli/src/run/agent.rs` 搬入并改造：

```rust
use std::io::Write;
use std::sync::Mutex;
use crate::cli_run::AnyStreamEvent;
use crate::stream::StreamEvent;
use crate::{ReActState, DupState, TotState, GotState, ToolCall, MessageChunk, MessageChunkKind};
use super::format::*;

pub struct StreamDisplayConfig {
    pub verbose: bool,
    pub display_max_len: usize,
    pub output_timestamp: bool,
    pub agent_display: Option<String>,
}

pub struct EventState {
    pub turn: u32,
    pub last_node: Option<String>,
    pub reply_started: bool,
    pub agent_display: Option<String>,
    pub total_prompt_tokens: u32,
    pub total_completion_tokens: u32,
}

impl EventState {
    pub fn new(agent_display: Option<String>) -> Self {
        Self {
            turn: 0,
            last_node: None,
            reply_started: false,
            agent_display,
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
        }
    }
}

/// 创建一个可直接传给 run_agent_with_options 的 on_event 回调，
/// 默认输出到 stdout/stderr。
pub fn create_stdio_event_callback(
    config: StreamDisplayConfig,
) -> Box<dyn FnMut(AnyStreamEvent) + Send> {
    let state = Mutex::new(EventState::new(config.agent_display));
    let display_max_len = config.display_max_len;
    let verbose = config.verbose;
    let output_timestamp = config.output_timestamp;

    Box::new(move |ev: AnyStreamEvent| {
        let mut s = match state.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        match &ev {
            AnyStreamEvent::React(e) => {
                on_event_react(e, &mut s, display_max_len, verbose, output_timestamp)
            }
            AnyStreamEvent::Dup(e) => {
                on_event_dup(e, &mut s, display_max_len, verbose, output_timestamp)
            }
            AnyStreamEvent::Tot(e) => {
                on_event_tot(e, &mut s, display_max_len, verbose, output_timestamp)
            }
            AnyStreamEvent::Got(e) => {
                on_event_got(e, &mut s, display_max_len, verbose, output_timestamp)
            }
        }
    })
}
```

搬入并改造的函数：
- `on_event_react` / `on_event_dup` / `on_event_tot` / `on_event_got` — 改为 `pub`，保持 `eprintln!` / `print!` 输出
- `log_node_enter` / `log_tools_used` — 改为 `pub`，保持 `eprintln!`
- `print_stream_chunk` — 改为 `pub`，保持 stdout/stderr 分流
- `format_context_limit` — 改为 `pub`
- `print_reply_timestamp` — 改为 `pub`（依赖 `chrono::Local`）

#### 1.4 在 `loom/src/lib.rs` 中注册模块

```rust
pub mod stream_display;
```

并导出工厂函数：
```rust
pub use stream_display::{create_stdio_event_callback, StreamDisplayConfig};
```

### Phase 2：LoomTool 接入

#### 2.1 `LoomTool` 增加字段（`loom/src/goal_runner/tool.rs`）

```rust
pub struct LoomTool {
    // ... 现有字段 ...
    any_stream_event_sender: Option<Arc<dyn Fn(AnyStreamEvent) + Send + Sync>>,
}

// 新增 builder
impl LoomTool {
    pub fn with_event_sender(
        mut self,
        sender: Arc<dyn Fn(AnyStreamEvent) + Send + Sync>,
    ) -> Self {
        self.any_stream_event_sender = Some(sender);
        self
    }
}
```

#### 2.2 `LoomTool::execute` 改造

```rust
async fn execute(&self, prompt: &str, working_dir: &Path) -> Result<TurnResult, ToolError> {
    use crate::cli_run::{RunCmd, RunCompletion, RunOptions};
    use crate::message::UserContent;

    // 构造 on_event：优先用外部 sender，否则用默认 stdio 输出
    let on_event: Option<Box<dyn FnMut(AnyStreamEvent) + Send>> =
        if let Some(ref sender) = self.any_stream_event_sender {
            let sender = sender.clone();
            Some(Box::new(move |ev: AnyStreamEvent| {
                sender(ev);
            }))
        } else {
            // 使用 loom 内置的 stdio 输出（和 CLI 一致）
            Some(crate::stream_display::create_stdio_event_callback(
                crate::stream_display::StreamDisplayConfig {
                    verbose: self.verbose,
                    display_max_len: 10000,
                    output_timestamp: false,
                    agent_display: None,
                },
            ))
        };

    let mut opts = RunOptions {
        message: UserContent::Text(prompt.to_string()),
        // ... 现有字段 ...
        any_stream_event_sender: self.any_stream_event_sender.clone(),
    };

    let result = crate::cli_run::run_agent_with_options(&opts, &RunCmd::React, on_event)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("loom agent error: {}", e)))?;

    // ... result 处理不变 ...
}
```

### Phase 3：GoalRunner 透传 sender

#### 3.1 `GoalRunner` 增加字段（`loom/src/goal_runner/runner.rs`）

```rust
pub struct GoalRunner {
    // ... 现有字段 ...
    any_stream_event_sender: Option<Arc<dyn Fn(AnyStreamEvent) + Send + Sync>>,
}

impl GoalRunner {
    pub fn with_event_sender(
        mut self,
        sender: Arc<dyn Fn(AnyStreamEvent) + Send + Sync>,
    ) -> Self {
        self.any_stream_event_sender = Some(sender);
        self
    }
}
```

#### 3.2 `resolve_tool` 传 sender

```rust
fn resolve_tool(
    tool_name: &str,
    db_path: &std::path::Path,
    working_dir: &std::path::Path,
    event_sender: &Option<Arc<dyn Fn(AnyStreamEvent) + Send + Sync>>,
) -> Result<Box<dyn CodingTool>, GoalError> {
    match tool_name {
        "loom" => {
            let mcp_config_path = write_mcp_config(db_path, working_dir)?;
            let mut tool = super::tool::LoomTool::new(
                "goal-session".to_string(),
                working_dir.to_path_buf(),
                mcp_config_path,
            );
            if let Some(sender) = event_sender {
                tool = tool.with_event_sender(sender.clone());
            }
            Ok(Box::new(tool))
        }
        // ShellTool 不变
    }
}
```

### Phase 4：CLI 侧去重

#### 4.1 `cli/src/run/agent.rs:271-287` 改为调用 loom

```rust
// 之前：自己构建 on_event 闭包
// 之后：
let on_event = loom::stream_display::create_stdio_event_callback(
    loom::stream_display::StreamDisplayConfig {
        verbose: opts.verbose,
        display_max_len: display_max_len,
        output_timestamp: opts.output_timestamp,
        agent_display: resolved_agent.as_ref()
            .map(|ra| format!("{} ({})", ra.name, ra.source)),
    },
);
let result = run_agent_with_options(opts, cmd, Some(on_event)).await?;
```

#### 4.2 删除 CLI 侧重复代码

`cli/src/run/agent.rs` 中删除：
- `EventState` struct
- `on_event_react` / `on_event_dup` / `on_event_tot` / `on_event_got`
- `log_node_enter` / `log_tools_used` / `print_stream_chunk`
- `format_context_limit` / `print_reply_timestamp`

改为从 `loom::stream_display` import。

`cli/src/run/display.rs` 删除（整个文件），改为 import `loom::stream_display::format::*`。

## 文件改动汇总

| 文件 | 改动 |
|------|------|
| `loom/src/stream_display/mod.rs` | **新建** — 模块入口 |
| `loom/src/stream_display/format.rs` | **新建** — 从 `cli/src/run/display.rs` 搬入 |
| `loom/src/stream_display/event_handler.rs` | **新建** — 从 `cli/src/run/agent.rs` 搬入事件处理 + 工厂函数 |
| `loom/src/lib.rs` | 新增 `pub mod stream_display` + 导出 |
| `loom/src/goal_runner/tool.rs` | `LoomTool` 加 `any_stream_event_sender` + builder + `execute` 改造 |
| `loom/src/goal_runner/runner.rs` | `GoalRunner` 加字段 + builder + `resolve_tool` 传 sender |
| `cli/src/run/agent.rs` | 改为调用 `loom::stream_display`，删除重复代码 |
| `cli/src/run/display.rs` | 删除（内容已搬入 loom） |
| `cli/src/run/mod.rs` | 更新 display import |

## 依赖变更

`loom/Cargo.toml` 需要新增：
- `chrono`（`print_reply_timestamp` 用到 `chrono::Local`）

检查 loom 是否已有 chrono 依赖。如果已有，无需新增。

## 风险点

### 1. `on_event` 与 `any_stream_event_sender` 的重复调用

`run_stream_with_config` 同时接受 `on_event` 和 `any_stream_event_sender`。
如果两者指向同一个闭包，事件会被发两次。

**解决方案**：
- `on_event`：接收当前 agent 的流式事件（用于显示）
- `any_stream_event_sender`：留给更深层子 agent 的 `invoke_agent` 冒泡
- 两者职责不同，不应该指向同一个闭包

### 2. `chrono` 依赖

`print_reply_timestamp` 使用 `chrono::Local`。需确认 loom 已有 chrono 依赖，
或者将 timestamp 功能改为可选 feature。

### 3. `print_stream_chunk` 写 stdout/stderr 的行为

`print_stream_chunk` 区分 Thinking → stderr，其他 → stdout。
这个行为在搬到 loom 后保持不变。但如果 telegram-bot 等场景使用 `create_stdio_event_callback`，
会直接写 stdout/stderr，这可能不是期望的。

**解决方案**：telegram-bot 等不使用 `create_stdio_event_callback`，
而是传入自己的 `any_stream_event_sender`。

### 4. 测试迁移

`cli/src/run/display.rs` 中的测试需要搬到 `loom/src/stream_display/format.rs`。
`cli/src/run/agent.rs` 中 `on_event_react` 的测试也需要搬迁。

## 实施步骤

1. 确认 loom 已有 chrono 依赖
2. 确认 `run_stream_with_config` 中 `on_event` 和 `any_stream_event_sender` 的调用关系
3. **Phase 1**：在 loom 中新建 `stream_display` 模块，搬入 display + event_handler
4. **Phase 2**：改造 `LoomTool`，加 sender + 默认 stdio 输出
5. **Phase 3**：改造 `GoalRunner`，透传 sender
6. **Phase 4**：CLI 侧改为调用 loom，删除重复代码
7. 编译 + 现有测试通过
8. 手动验证 goal_runner 的中间输出
