# Phase 4: 集成与优化实现方案

## 概述

Phase 4 将 TUI 与现有 Agent 系统集成，并添加高级功能 - ^Z 暂停/恢复、桌面通知、以及整体优化。

**目标**：完整对话体验，与传统模式无缝切换，高级交互功能可用。

**预计文件**：4-6 个文件，~800 行代码

---

## 1. 文件清单

| 文件 | 职责 | 预计行数 | 依赖 |
|------|------|----------|------|
| `tui/agent.rs` | Agent 集成适配 | ~250 | run/agent.rs, stream-event |
| `tui/job_control.rs` | ^Z 暂停/恢复 | ~200 | crossterm, tokio |
| `tui/notification.rs` | 桌面通知 | ~150 | notify-rust |
| `main.rs` | 入口集成 | ~100 | 以上所有 |

---

## 2. Agent 集成 (`tui/agent.rs`)

### 2.1 通道适配

```rust
use tokio::sync::mpsc;
use stream_event::StreamEvent;

/// Agent 事件类型
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
    /// 完成
    Completed,
    /// 错误
    Error(String),
}

/// 创建 Agent 通道
pub fn create_agent_channel() -> (mpsc::Sender<AgentEvent>, mpsc::Receiver<AgentEvent>) {
    mpsc::channel(256)
}

/// 将 StreamEvent 回调适配为 AgentEvent
pub fn create_stream_callback(
    tx: mpsc::Sender<AgentEvent>,
) -> impl FnMut(StreamEvent<serde_json::Value>) + Send {
    move |event| {
        let agent_event = match event {
            StreamEvent::TextDelta { content, .. } => {
                AgentEvent::TextDelta(content)
            }
            StreamEvent::ReasoningDelta { content, .. } => {
                AgentEvent::ReasoningDelta(content)
            }
            StreamEvent::ToolCall { call_id, name, arguments } => {
                AgentEvent::ToolCall { call_id, name, arguments }
            }
            StreamEvent::ToolStart { call_id, name } => {
                AgentEvent::ToolStart { call_id, name }
            }
            StreamEvent::ToolOutput { call_id, name, content } => {
                AgentEvent::ToolOutput { call_id, name, content }
            }
            StreamEvent::ToolEnd { call_id, name, result, is_error, raw_result } => {
                AgentEvent::ToolEnd { call_id, name, result, is_error, raw_result }
            }
            _ => return,
        };
        let _ = tx.try_send(agent_event);
    }
}
```

### 2.2 运行函数

```rust
/// 在 TUI 模式下运行 Agent
pub async fn run_agent_tui(
    opts: &RunOptions,
    cmd: &RunCmd,
    agent_tx: mpsc::Sender<AgentEvent>,
    cancel: CancellationToken,
) -> Result<(), Box<dyn std::error::Error>> {
    // 创建回调
    let callback = create_stream_callback(agent_tx.clone());
    let callback = Arc::new(Mutex::new(callback));

    // 运行 agent（与现有 run_cli_turn 兼容）
    let result = run_cli_turn(opts, cmd, callback).await;

    // 发送完成事件
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

---

## 3. ^Z 暂停/恢复 (`tui/job_control.rs`)

### 3.1 设计原则

- 仅在 Unix 系统上支持
- 使用 `tokio::signal::unix` 捕获 SIGTSTP
- 恢复时自动刷新终端状态

### 3.2 核心实现

```rust
use std::sync::Arc;
use tokio::sync::Notify;

/// ^Z 暂停/恢复管理器
pub struct JobControl {
    /// 暂停通知
    suspend: Arc<Notify>,
    /// 恢复通知
    resume: Arc<Notify>,
    /// 是否已暂停
    suspended: bool,
}

impl JobControl {
    pub fn new() -> Self {
        Self {
            suspend: Arc::new(Notify::new()),
            resume: Arc::new(Notify::new()),
            suspended: false,
        }
    }

    /// 获取暂停信号（用于 tokio::select!）
    pub fn suspend_signal(&self) -> Arc<Notify> {
        self.suspend.clone()
    }

    /// 获取恢复信号
    pub fn resume_signal(&self) -> Arc<Notify> {
        self.resume.clone()
    }

    /// 暂停当前进程
    pub fn suspend(&self) -> Result<()> {
        self.suspended = true;

        // 1. 恢复终端状态
        execute!(stdout(), DisableBracketedPaste)?;
        execute!(stdout(), Show)?;
        disable_raw_mode()?;

        // 2. 发送 SIGTSTP
        // 安全: 标准 Unix 信号操作
        unsafe {
            libc::raise(libc::SIGTSTP);
        }

        // 3. 恢复后，重新初始化终端
        enable_raw_mode()?;
        execute!(stdout(), EnableBracketedPaste)?;
        execute!(stdout(), Hide)?;

        self.suspended = false;

        // 4. 通知恢复
        self.resume.notify_waiters();

        Ok(())
    }

    /// 是否已暂停
    pub fn is_suspended(&self) -> bool {
        self.suspended
    }
}

/// 启用 ^Z 信号处理
pub fn setup_signal_handler() -> Result<()> {
    // 忽略 SIGTSTP（由 JobControl 手动处理）
    // 安全: 标准信号处理设置
    unsafe {
        libc::signal(libc::SIGTSTP, libc::SIG_IGN);
    }
    Ok(())
}
```

---

## 4. 桌面通知 (`tui/notification.rs`)

### 4.1 设计原则

- 轻量级，使用 `notify-rust` crate
- 仅当终端失焦时发送通知
- 可配置通知条件（通过配置控制）

### 4.2 核心实现

```rust
/// 通知类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NotificationType {
    /// AI 回复完成
    ReplyComplete,
    /// 需要审批
    NeedApproval,
    /// 错误
    Error,
}

/// 桌面通知管理器
pub struct NotificationManager {
    /// 终端是否处于焦点
    terminal_focused: Arc<AtomicBool>,
    /// 是否启用通知
    enabled: bool,
}

impl NotificationManager {
    pub fn new(enabled: bool) -> Self {
        Self {
            terminal_focused: Arc::new(AtomicBool::new(true)),
            enabled,
        }
    }

    /// 获取终端焦点状态（用于跨线程共享）
    pub fn focus_state(&self) -> Arc<AtomicBool> {
        self.terminal_focused.clone()
    }

    /// 发送通知
    pub fn notify(&self, notif_type: NotificationType) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        // 如果终端在焦点中，不发送通知
        if self.terminal_focused.load(Ordering::Relaxed) {
            return Ok(());
        }

        let (title, body) = match notif_type {
            NotificationType::ReplyComplete => {
                ("Loom TUI", "AI 回复已完成")
            }
            NotificationType::NeedApproval => {
                ("Loom TUI", "需要你的审批")
            }
            NotificationType::Error => {
                ("Loom TUI", "发生错误")
            }
        };

        #[cfg(target_os = "macos")]
        {
            // macOS: 使用 osascript 发送通知
            std::process::Command::new("osascript")
                .arg("-e")
                .arg(format!(
                    r#"display notification "{}" with title "{}""#,
                    body, title
                ))
                .output()?;
        }

        Ok(())
    }
}
```

---

## 5. 主入口集成

### 5.1 main.rs 修改

```rust
// main.rs 的 cli 命令部分
#[derive(Parser)]
struct Args {
    /// 交互式模式（TUI）
    #[arg(short = 'i', long = "interactive")]
    interactive: bool,

    /// 其他参数...
    #[command(flatten)]
    run: RunArgs,
}

async fn run() -> Result<()> {
    let args = Args::parse();

    if args.interactive {
        #[cfg(feature = "tui")]
        {
            let mut app = tui::App::new()?;
            app.run().await?;
            return Ok(());
        }
        #[cfg(not(feature = "tui"))]
        {
            eprintln!("TUI 模式未启用。请使用 `cargo build --features tui` 重新构建。");
            return Ok(());
        }
    }

    // 传统模式
    run_traditional(args).await
}
```

### 5.2 Cargo.toml 最终依赖

```toml
[features]
default = []
tui = [
    "ratatui",
    "crossterm",
    "crossterm/event-stream",
    "dep:notify-rust",
]

[dependencies]
ratatui = { version = "0.28", optional = true, features = ["crossterm"] }
crossterm = { version = "0.28", optional = true, features = ["event-stream", "bracketed-paste"] }
notify-rust = { version = "4", optional = true }

[target.'cfg(unix)'.dependencies]
libc = { version = "0.2", optional = true }
```

---

## 6. 整体集成测试

### 6.1 端到端测试

| 测试场景 | 操作 | 预期结果 |
|----------|------|----------|
| TUI 模式启动 | `cargo run --features tui -- -i` | 进入 TUI 模式，显示输入框 |
| 输入并提交 | 输入文本 → Enter | 消息发送到 AI，显示状态 |
| 流式输出 | → AI 回复 | 内容逐字显示 |
| 审批弹窗 | AI 请求工具调用 | 弹出审批视图 |
| 允许审批 | 按 Y | 审批通过，继续执行 |
| 拒绝审批 | 按 N | 审批拒绝 |
| 中断流程 | Ctrl+C | 中断当前操作 |
| 退出 TUI | Ctrl+D | 退出 TUI，终端恢复 |
| ^Z 暂停 | Ctrl+Z | 暂停进程，恢复后重新进入 TUI |
| 传统模式 | `cargo run -- --prompt "hello"` | 正常输出，不受影响 |

### 6.2 回归测试

| 测试项 | 说明 |
|--------|------|
| 传统模式输出 | 与 TUI 模式无关，输出格式不变 |
| 管道模式 | `echo "hello" | cargo run` 正常工作 |
| 非 TTY 环境 | 自动降级到传统模式 |
| 构建时间 | TUI feature 不影响传统模式编译时间 |

---

## 7. 性能优化

### 7.1 渲染优化

| 优化项 | 方法 | 预期效果 |
|--------|------|----------|
| 帧率控制 | 每 100ms 最多重绘一次 | 减少 CPU 占用 |
| 增量渲染 | 只渲染变化的部分 | 减少 buffer 操作 |
| 虚拟滚动 | 只渲染可见历史行 | 大对话历史不卡顿 |
| 延迟渲染 | 快速连续更新合并为一次绘制 | 避免闪烁 |

### 7.2 内存优化

| 优化项 | 方法 | 预期效果 |
|--------|------|----------|
| 历史截断 | 超过 N 行的历史滚动到终端 | 减少内存占用 |
| 流式缓冲 | 控制 mpsc channel 大小 | 避免内存溢出 |
| 懒加载 | 历史内容按需加载 | 减少初始内存占用 |

---

## 8. 交付标准

- [x] `tui/agent.rs` Agent 事件适配
- [x] `tui/job_control.rs` ^Z 暂停/恢复
- [x] `tui/notification.rs` 桌面通知
- [x] `main.rs` 入口集成
- [x] `Cargo.toml` 最终依赖配置
- [ ] 完整对话体验：输入 → 提交 → AI 处理 → 结果显示
- [ ] ^Z 暂停/恢复
- [ ] 桌面通知（终端失焦时）
- [ ] 传统模式完全不受影响
- [ ] 端到端测试通过