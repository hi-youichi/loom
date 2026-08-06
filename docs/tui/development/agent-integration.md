# Agent 集成方案

## 概述

本文档详细描述 Loom TUI 与现有 Agent 系统的集成方案。这是 TUI 开发中最关键的集成点，直接影响整体架构的可行性和可维护性。

**核心问题**：如何将现有 `run/agent.rs` 的回调模式（`StreamCallback`）适配为 TUI 的事件驱动模式（`tokio::select!` + `mpsc` channel）。

---

## 1. 现有架构分析

### 1.1 当前回调模式

```rust
// apps/cli/src/run/agent.rs
type StreamCallback<S> = Arc<Mutex<dyn FnMut(StreamEvent<S>) + Send>>;

pub async fn run_cli_turn(
    opts: &RunOptions,
    cmd: &RunCmd,
    callback: StreamCallback<serde_json::Value>,
) -> Result<RunOutput, RunError> {
    // 1. 创建 agent
    let agent = create_agent(opts, cmd)?;

    // 2. 运行 agent
    let result = agent.run(|event| {
        let mut cb = callback.lock().unwrap();
        cb(event);
    }).await;

    // 3. 返回结果
    result
}
```

### 1.2 当前使用方式

```rust
// display/event_handler.rs (传统模式)
let callback = |event: StreamEvent| {
    // 格式化输出到 stderr
    format_and_print(event);
};

// run_flow.rs
let output = run_cli_turn(&opts, &cmd, Arc::new(Mutex::new(callback))).await;
```

### 1.3 关键约束

| 约束 | 说明 |
|------|------|
| 回调类型 | `Arc<Mutex<dyn FnMut(StreamEvent<S>) + Send>>` |
| 回调调用 | 在 agent 运行线程中同步调用 |
| 事件类型 | `StreamEvent<serde_json::Value>` |
| 运行函数 | `run_cli_turn()` 接受 callback 参数 |
| 返回值 | `RunOutput` 或 `RunError` |

---

## 2. TUI 适配方案

### 2.1 通道适配

```rust
// tui/agent.rs

use tokio::sync::mpsc;
use std::sync::{Arc, Mutex};
use stream_event::StreamEvent;

/// TUI Agent 事件（简化版，按需处理的事件类型）
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// 文本 delta（流式输出）
    TextDelta(String),
    /// 推理 delta
    ReasoningDelta(String),
    /// 工具调用
    ToolCall {
        call_id: Option<String>,
        name: String,
        arguments: serde_json::Value,
    },
    /// 工具开始执行
    ToolStart {
        call_id: Option<String>,
        name: String,
    },
    /// 工具输出
    ToolOutput {
        call_id: Option<String>,
        name: String,
        content: String,
    },
    /// 工具结束
    ToolEnd {
        call_id: Option<String>,
        name: String,
        result: String,
        is_error: bool,
        raw_result: Option<String>,
    },
    /// Agent 完成
    Completed,
    /// Agent 错误
    Error(String),
}

/// 创建 Agent 通道对
pub fn create_agent_channel() -> (mpsc::Sender<AgentEvent>, mpsc::Receiver<AgentEvent>) {
    mpsc::channel(256)
}

/// 将回调适配为 StreamEvent → AgentEvent 转换
fn convert_event(event: StreamEvent<serde_json::Value>) -> Option<AgentEvent> {
    match event {
        StreamEvent::TextDelta { content, .. } => Some(AgentEvent::TextDelta(content)),
        StreamEvent::ReasoningDelta { content, .. } => Some(AgentEvent::ReasoningDelta(content)),
        StreamEvent::ToolCall { call_id, name, arguments } => {
            Some(AgentEvent::ToolCall { call_id, name, arguments })
        }
        StreamEvent::ToolStart { call_id, name } => {
            Some(AgentEvent::ToolStart { call_id, name })
        }
        StreamEvent::ToolOutput { call_id, name, content } => {
            Some(AgentEvent::ToolOutput { call_id, name, content })
        }
        StreamEvent::ToolEnd { call_id, name, result, is_error, raw_result } => {
            Some(AgentEvent::ToolEnd { call_id, name, result, is_error, raw_result })
        }
        _ => None, // 忽略其他事件类型
    }
}

/// 创建流回调适配器
pub fn create_stream_callback(
    tx: mpsc::Sender<AgentEvent>,
) -> impl FnMut(StreamEvent<serde_json::Value>) + Send {
    move |event| {
        if let Some(agent_event) = convert_event(event) {
            // 使用 try_send 以避免阻塞 agent 运行线程
            let _ = tx.try_send(agent_event);
        }
    }
}
```

### 2.2 运行函数

```rust
/// 在 TUI 模式下运行 Agent 单轮
pub async fn run_agent_tui_turn(
    opts: &RunOptions,
    cmd: &RunCmd,
    agent_tx: mpsc::Sender<AgentEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. 创建回调
    let callback = create_stream_callback(agent_tx.clone());
    let callback = Arc::new(Mutex::new(callback));

    // 2. 运行 agent（复用现有函数）
    let result = run_cli_turn(opts, cmd, callback).await;

    // 3. 发送完成/错误事件
    match result {
        Ok(_) => {
            let _ = agent_tx.send(AgentEvent::Completed).await;
        }
        Err(e) => {
            let _ = agent_tx.send(AgentEvent::Error(e.to_string())).await;
        }
    }

    Ok(())
}
```

### 2.3 中断支持

```rust
/// 支持中断的 Agent 运行函数
pub async fn run_agent_tui_with_cancel(
    opts: &RunOptions,
    cmd: &RunCmd,
    agent_tx: mpsc::Sender<AgentEvent>,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<(), Box<dyn std::error::Error>> {
    let callback = create_stream_callback(agent_tx.clone());
    let callback = Arc::new(Mutex::new(callback));

    // 在另一个线程中运行 agent
    let cancel_clone = cancel.clone();
    let handle = tokio::task::spawn_blocking(move || {
        // 创建 tokio runtime 用于 agent 运行
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            let result = run_cli_turn(opts, cmd, callback).await;
            match result {
                Ok(_) => {
                    if !cancel_clone.is_cancelled() {
                        let _ = agent_tx.send(AgentEvent::Completed).await;
                    }
                }
                Err(e) => {
                    let _ = agent_tx.send(AgentEvent::Error(e.to_string())).await;
                }
            }
        })
    });

    // 等待完成或取消
    tokio::select! {
        _ = cancel.cancelled() => {
            // 取消操作
            handle.abort();
            Ok(())
        }
        result = handle => {
            result?;
            Ok(())
        }
    }
}
```

---

## 3. 事件处理流程

### 3.1 完整数据流

```
用户输入
  → App::handle_submit(content)
  → 创建 AgentEvent channel
  → 创建 CancellationToken
  → 在后台 task 中运行 run_agent_tui_turn()
  → Agent 开始运行
  → Agent 产生 StreamEvent
  → StreamCallback 被调用
  → convert_event() → AgentEvent
  → tx.try_send(AgentEvent)
  → App 主循环的 rx.recv() 收到事件
  → App::handle_agent_event(event)
  → 更新 UI 状态
  → 渲染
```

### 3.2 App 侧处理

```rust
// tui/app.rs
impl App {
    /// 处理 Agent 事件
    fn handle_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta(content) => {
                // 更新流式输出 cell
                if let Some(active) = &mut self.active_cell {
                    active.append_text(&content);
                } else {
                    // 创建新的流式输出 cell
                    let mut cell = StreamingCell::new();
                    cell.append_text(&content);
                    self.active_cell = Some(cell);
                }
                // 状态更新
                self.status = AiStatus::Thinking;
            }

            AgentEvent::ReasoningDelta(_content) => {
                // 推理过程：显示 spinner + 状态
                self.status = AiStatus::Thinking;
            }

            AgentEvent::ToolCall { name, arguments, .. } => {
                // 工具调用：根据审批策略决定是否弹出审批
                match self.approval_strategy {
                    ApprovalStrategy::Always => {
                        // 直接执行，显示工具调用状态
                        self.status = AiStatus::Executing;
                    }
                    _ => {
                        // 弹出审批
                        let request = ApprovalRequest::ToolCall {
                            tool_name: name,
                            args: arguments,
                        };
                        self.pane_stack.push(Box::new(ApprovalPane::new(request)));
                        self.state = self.state.transition(
                            AppEvent::RequestApproval
                        ).unwrap();
                    }
                }
            }

            AgentEvent::ToolStart { name, .. } => {
                // 工具开始执行
                self.status = AiStatus::Executing;
                // 添加工具调用 cell
                if let Some(active) = &mut self.active_cell {
                    // 在流式输出中插入工具调用标记
                }
            }

            AgentEvent::ToolOutput { content, .. } => {
                // 工具输出（增量）
                if let Some(active) = &mut self.active_cell {
                    // 更新工具调用输出
                }
            }

            AgentEvent::ToolEnd { name, result, is_error, .. } => {
                // 工具完成
                // 将工具调用结果添加到历史
                self.history.push(HistoryCell::ToolCall {
                    tool_name: name,
                    args: serde_json::Value::Null,
                    result: Some(result),
                    status: if is_error {
                        ToolStatus::Failed
                    } else {
                        ToolStatus::Completed
                    },
                });
            }

            AgentEvent::Completed => {
                // Agent 完成，将流式 cell 转为历史 cell
                if let Some(active) = self.active_cell.take() {
                    self.history.push(HistoryCell::AssistantMessage {
                        content: active.content,
                        timestamp: chrono::Utc::now(),
                    });
                }
                self.state = self.state.transition(AppEvent::Completed).unwrap();
                self.status = AiStatus::Idle;
                // 发送桌面通知（如果终端失焦）
                self.notification.notify(NotificationType::ReplyComplete).ok();
            }

            AgentEvent::Error(e) => {
                self.error = Some(e);
                self.state = self.state.transition(AppEvent::Error).unwrap();
                self.status = AiStatus::Error;
                self.notification.notify(NotificationType::Error).ok();
            }
        }
    }
}
```

---

## 4. 与现有 REPL 的关系

### 4.1 现有 REPL (`repl.rs`)

当前 REPL 通过 `run_one_turn()` 实现单轮交互：

```rust
pub async fn run_one_turn(
    opts: &RunOptions,
    cmd: &Command,
    stream_out: EventSink,
) -> Result<RunOutput, RunError> {
    let run_cmd = cmd_to_runcmd(cmd);
    run_cli_turn(opts, &run_cmd, stream_out).await
}
```

### 4.2 TUI 模式替代

TUI 模式将**完全替代** REPL 的交互功能：

| 功能 | REPL (repl.rs) | TUI (tui/agent.rs) |
|------|---------------|-------------------|
| 输入 | `stdin` 行读取 | 交互式输入框 |
| 输出 | ANSI 文本 | ratatui 渲染 |
| 循环 | `while` 循环 | `tokio::select!` 主循环 |
| 事件 | 同步等待 | 异步事件流 |
| 审批 | 不支持 | 支持交互式审批 |

### 4.3 共存策略

- TUI 模式和 REPL 模式通过 `--interactive` 参数区分
- 传统 `--interactive` 行为保持兼容（当未启用 TUI feature 时）
- 未来可完全移除 REPL，由 TUI 替代

---

## 5. 错误处理

### 5.1 错误场景

| 场景 | 处理方式 |
|------|----------|
| Agent 运行失败 | 发送 `AgentEvent::Error`，显示错误信息 |
| Channel 满 | 使用 `try_send`，丢弃最旧的事件 |
| Channel 关闭 | Agent 运行线程正常退出 |
| 取消中断 | 中断 Agent 运行，清理状态 |
| Panic | panic hook 恢复终端，退出 |

### 5.2 Channel 管理

```rust
// 通道容量
const AGENT_CHANNEL_SIZE: usize = 256;

// 发送策略
impl AgentEvent {
    fn try_send_to(tx: &mpsc::Sender<Self>, event: Self) {
        match tx.try_send(event) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(event)) => {
                // Channel 满：丢弃最旧的事件
                // 这是可接受的，因为流式输出可以跳帧
                tracing::warn!("Agent event channel full, dropping event");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // Channel 关闭：停止发送
                tracing::debug!("Agent event channel closed");
            }
        }
    }
}
```

---

## 6. 测试策略

### 6.1 单元测试

| 测试 | 说明 |
|------|------|
| `test_convert_event` | 验证 StreamEvent → AgentEvent 转换正确 |
| `test_agent_channel` | 验证 mpsc channel 发送接收 |
| `test_cancel_token` | 验证取消信号正确传递 |

### 6.2 集成测试

| 测试 | 说明 |
|------|------|
| `test_tui_agent_roundtrip` | 模拟 Agent 运行，验证事件流正确 |
| `test_tui_agent_cancel` | 验证取消 Agent 运行 |
| `test_tui_agent_error` | 验证 Agent 错误处理 |

### 6.3 手动测试

| 测试 | 操作 |
|------|------|
| 提交消息 | 输入文本 → Enter → 观察输出 |
| 中断回复 | AI 回复中按 Ctrl+C → 观察中断 |
| 工具调用审批 | 触发工具调用 → 观察审批弹窗 |
| 错误恢复 | 触发错误 → 观察错误显示 → 恢复