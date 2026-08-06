# Loom TUI 交互文档：通知系统与系统集成

## 概述

本文档描述 Loom TUI 系统级交互功能，包括桌面通知、进程管理、终端 Resize 处理、中断处理与会话管理。这些功能确保用户能够在不同场景下获得及时响应，系统状态保持一致，交互体验稳定可靠。

---

## 1. 桌面通知系统

### 1.1 通知架构

```
系统事件触发
    │
    ▼
┌─────────────────────────────────────────────────────────────┐
│                    通知调度器                                 │
│                                                             │
│  1. 通知条件判断：                                           │
│     - 终端是否失焦？(terminal_focused)                       │
│     - 用户通知偏好？(Unfocused / Always / Never)             │
│     - 通知频率限制？(5秒内不重复)                            │
│                                                             │
│  2. 平台后端选择：                                           │
│     - macOS: 系统通知中心 (Notification Center)              │
│     - Linux: notify-send / dbus                            │
│     - Windows: toast notification                           │
│                                                             │
│  3. 通知发送：                                               │
│     - 标题: "Loom TUI"                                      │
│     - 正文: 事件描述                                        │
│     - 失败时自动禁用该后端                                  │
└─────────────────────────────────────────────────────────────┘
```

### 1.2 通知类型

| 通知类型 | 触发条件 | 通知内容 | 默认设置 | 可配置性 |
|----------|----------|----------|----------|----------|
| `ReplyComplete` | AI 回复完成 | "AI 已完成回复" | Unfocused | ✅ 可关闭 |
| `ApprovalRequired` | AI 请求审批 | "AI 请求修改文件: xxx" | Always | ✅ 必须 |
| `Error` | 系统错误 | "发生错误: xxx" | Unfocused | ✅ 可关闭 |
| `SubTaskComplete` | 子代理完成 | "子任务已完成" | Never | ❌ 默认关闭 |

### 1.3 通知条件配置

```
NotificationCondition 枚举：
  - Unfocused (默认): 仅在终端失焦时通知
  - Always: 始终发送通知
  - Never: 永不发送通知

实现方式：
  - 用户配置: ~/.loom/config.toml [tui.notifications]
  - 运行时状态: Arc<AtomicBool> terminal_focused
  - 焦点检测: crossterm EventStream::FocusGained/Lost
```

### 1.4 通知频率控制

```
频率限制策略：
  1. 按通知类型分组，每个类型独立计时
  2. 同一类型通知 5 秒内只发送一次
  3. 超出限制的通知被丢弃，但不影响后续通知

实现方式：
  - HashMap<NotificationType, Instant> last_notified
  - 每次发送前检查: now - last_notified.get(&type) >= Duration::from_secs(5)
  - 使用 tokio::time::Instant 高精度计时
```

### 1.5 平台实现细节

#### macOS
```rust
// 使用 notify-rs crate
Notification::new()
    .summary("Loom TUI")
    .body(&message)
    .timeout(5000)
    .show()?;
```

#### Linux
```rust
// 调用 notify-send 命令
Command::new("notify-send")
    .arg("Loom TUI")
    .arg(&message)
    .arg("-t")
    .arg("5000")
    .spawn()?;
```

#### Windows
```rust
// 使用 win10toast crate
show_toast("Loom TUI", &message, Duration::from_secs(5))
```

### 1.6 失败处理与降级

```
通知发送失败处理：
  1. 捕获通知后端异常
  2. 记录错误日志 (warning 级别)
  3. 自动禁用失败的后端 (设置 backend_enabled = false)
  4. 后续尝试使用其他可用的后端

降级策略：
  - macOS/Linux: 通知失败 → 降级到 stderr 输出
  - Windows: toast 失败 → 降级到系统托盘图标闪烁
  - 所有平台: 最终降级到终端内状态提示
```

---

## 2. 进程管理

### 2.1 暂停/恢复 (^Z 暂停)

#### 暂停流程
```
用户按 ^Z
    │
    ▼
SuspendContext::prepare_suspend_action()
    │
    ├── 1. 保存当前终端状态 (raw mode, cursor position)
    ├── 2. 恢复终端到正常模式 (restore terminal mode)
    ├── 3. 暂停事件轮询 (EventBroker::pause())
    ├── 4. 清理屏幕显示 (清除 inline viewport)
    └── 5. 发送 SIGSTOP 到自己进程
        │
        ▼
    进程被操作系统挂起
```

#### 恢复流程
```
用户输入 fg (或其他恢复机制)
    │
    ▼
进程收到 SIGCONT 信号
    │
    ▼
┌─────────────────────────────────────────────────────────────┐
│  恢复流程：                                                  │
│  1. 重新设置终端为 raw mode                                 │
│  2. 恢复光标位置                                             │
│  3. 重新计算 inline viewport 位置                           │
│  4. 恢复事件轮询 (EventBroker::resume())                    │
│  5. 触发全量重绘 (force redraw)                             │
│  6. 继续正常交互                                             │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 SuspendContext 实现

```rust
pub struct SuspendContext {
    original_terminal_mode: crossterm::terminal::Mode,
    cursor_position: Option<(u16, u16)>,
    viewport_state: ViewportState,
    event_broker: Arc<EventBroker>,
}

impl SuspendContext {
    pub async fn prepare_suspend_action(&self) -> anyhow::Result<()> {
        // 保存当前终端状态
        self.original_terminal_mode = crossterm::terminal::mode()?;
        self.cursor_position = crossterm::cursor::position().ok();
        
        // 恢复正常模式
        crossterm::terminal::disable_raw_mode()?;
        
        // 暂停事件轮询
        self.event_broker.pause().await?;
        
        // 清理屏幕
        self.cleanup_viewport()?;
        
        Ok(())
    }
    
    pub async fn resume(&self) -> anyhow::Result<()> {
        // 恢复 raw mode
        crossterm::terminal::enable_raw_mode()?;
        
        // 恢复光标位置
        if let Some((col, row)) = self.cursor_position {
            crossterm::cursor::move_to(col, row)?;
        }
        
        // 恢复 viewport
        self.restore_viewport()?;
        
        // 恢复事件轮询
        self.event_broker.resume().await?;
        
        // 触发重绘
        self.force_redraw().await?;
        
        Ok(())
    }
}
```

### 2.3 运行外部程序

```rust
impl Tui {
    pub async fn with_restored<F, R>(&self, f: F) -> anyhow::Result<R>
    where
        F: FnOnce() -> R,
    {
        // 1. 暂停事件轮询
        self.event_broker.pause().await?;
        
        // 2. 恢复终端到正常模式
        let original_mode = crossterm::terminal::mode()?;
        crossterm::terminal::disable_raw_mode()?;
        
        // 3. 清理屏幕
        self.cleanup_viewport()?;
        
        // 4. 执行外部程序
        let result = f();
        
        // 5. 恢复终端模式
        crossterm::terminal::set_mode(original_mode)?;
        
        // 6. 恢复 viewport
        self.restore_viewport()?;
        
        // 7. 恢复事件轮询
        self.event_broker.resume().await?;
        
        // 8. 触发重绘
        self.force_redraw().await?;
        
        Ok(result)
    }
}
```

### 2.4 外部程序使用场景

```
需要运行外部程序的场景：
  1. 用户触发外部编辑器 (vim, emacs)
  2. 执行 shell 命令 (git, npm, docker)
  3. 启动调试工具 (gdb, lldb)
  4. 打开浏览器查看文档
  5. 执行系统级操作 (包管理器，系统更新)

用户体验：
  - TUI 界面自动隐藏，终端回到正常状态
  - 外部程序拥有完整的终端控制权
  - 程序结束后 TUI 自动恢复
  - 终端状态完全恢复，无任何残留影响
```

---

## 3. 终端 Resize 处理

### 3.1 Resize 检测与处理

```
用户调整终端窗口大小
    │
    ▼
crossterm EventStream 检测到 Resize 事件
    │
    ▼
TuiEvent::Resize(width, height)
    │
    ▼
App::handle_resize(width, height)
    │
    ├── 1. 更新终端尺寸: terminal_size = (width, height)
    ├── 2. 重新计算 viewport 位置
    ├── 3. 调整 UI 布局 (重新分配高度)
    ├── 4. 触发 transcript_reflow 处理历史内容
    └── 5. 触发全量重绘 (force redraw)
```

### 3.2 InlineViewport 调整

```rust
fn update_inline_viewport_for_resize_reflow(
    viewport: &mut InlineViewport,
    old_size: (u16, u16),
    new_size: (u16, u16),
) {
    match (new_size.1.cmp(&old_size.1), &viewport.alignment) {
        // 窗口变小：上滚历史区域
        (Ordering::Less, ViewportAlignment::Top) => {
            // viewport 顶部对齐，不需要调整
        }
        (Ordering::Less, ViewportAlignment::Bottom) => {
            // viewport 底部对齐，向上移动
            let height_diff = old_size.1 - new_size.1;
            viewport.row = viewport.row.saturating_sub(height_diff);
        }
        
        // 窗口变大：扩展渲染区域
        (Ordering::Greater, ViewportAlignment::Top) => {
            // viewport 顶部对齐，向下扩展
            let height_diff = new_size.1 - old_size.1;
            viewport.height += height_diff;
        }
        (Ordering::Greater, ViewportAlignment::Bottom) => {
            // viewport 底部对齐，向上扩展
            let height_diff = new_size.1 - old_size.1;
            viewport.row = viewport.row.saturating_sub(height_diff);
            viewport.height += height_diff;
        }
        
        // 窗口宽度变化：重新计算文本换行
        (Ordering::Equal, _) | (_, _) => {
            // 重新计算文本布局
            viewport.needs_reflow = true;
        }
    }
    
    // 确保 viewport 不超出终端边界
    viewport.height = viewport.height.min(new_size.1);
    viewport.row = viewport.row.min(new_size.1 - viewport.height);
}
```

### 3.3 Transcript Reflow 处理

```rust
pub struct TranscriptReflow {
    history_cells: Vec<HistoryCell>,
    pending_history_lines: Vec<String>,
}

impl TranscriptReflow {
    pub fn handle_resize(&mut self, new_width: u16) -> anyhow::Result<()> {
        // 1. 重新计算所有历史 cell 的布局
        for cell in &mut self.history_cells {
            cell.reflow(new_width)?;
        }
        
        // 2. 重新处理待插入的历史行
        for line in &mut self.pending_history_lines {
            *line = self.wrap_text(line, new_width)?;
        }
        
        // 3. 更新 viewport 内容
        self.update_viewport_content()?;
        
        Ok(())
    }
    
    fn wrap_text(&self, text: &str, width: u16) -> String {
        // 实现文本换行逻辑，考虑中文字符和 ANSI 转义序列
        textwrap::wrap(text, width as usize).join("\n")
    }
}
```

### 3.4 光标位置保持

```
光标位置保持策略：
  1. 在 resize 前保存当前光标位置 (cursor_pos)
  2. 重新计算 viewport 后，重新计算光标在 viewport 中的相对位置
  3. 确保 cursor_pos 仍在有效区域内
  4. 如果超出范围，调整到最接近的有效位置

实现方式：
  - 保存: (relative_row, relative_col) = cursor_pos - viewport.start
  - 恢复: cursor_pos = viewport.start + (relative_row, relative_col)
  - 边界检查: 确保 cursor_pos 在 viewport 范围内
```

### 3.5 Resize 边缘情况处理

```
边缘情况处理：
  1. 终端缩小到无法容纳最小 viewport:
     - 缩小 viewport 到最小尺寸
     - 显示错误提示 "终端尺寸过小"
     - 暂停交互，等待用户放大终端
     
  2. 快速连续 resize:
     - 使用防抖机制 (debounce)
     - 只处理最后一次 resize 事件
     - 避免频繁重绘导致性能问题
     
  3. Resize 事件丢失:
     - 定期检查终端实际尺寸
     - 发现不一致时触发 resize 处理
     - 使用 tokio::time::interval 定时检查
```

---

## 4. Ctrl+C 中断系统

### 4.1 双击强制退出机制

```
第一次 Ctrl+C:
    │
    ▼
┌─────────────────────────────────────────────────────────────┐
│  1. 检查当前状态                                              │
│  2. 如果有活跃操作，触发软中断                              │
│  3. 停止当前 AI 操作 (RunCancellation)                      │
│  4. 清理未完成的流式输出                                     │
│  5. 显示中断提示: "操作已中断，再次按 Ctrl+C 强制退出"        │
│  6. 启动 2 秒计时器                                          │
│  7. 状态转换: Current → Interrupted                         │
└─────────────────────────────────────────────────────────────┘
    │
    ▼
2 秒内再次按 Ctrl+C:
    │
    ▼
┌─────────────────────────────────────────────────────────────┐
│  1. 设置 force_quit 标志                                     │
│  2. 发送停止信号到所有运行中的任务                           │
│  3. 清理所有异步任务                                         │
│  4. 恢复终端状态                                             │
│  5. 退出进程 (exit code: 1)                                  │
└─────────────────────────────────────────────────────────────┘
```

### 4.2 Ctrl+C 处理实现

```rust
pub struct CtrlCHandler {
    last_ctrl_c_time: Option<Instant>,
    force_quit_flag: Arc<AtomicBool>,
    cancel_token: Arc<RunCancellation>,
}

impl CtrlCHandler {
    pub async fn handle_ctrl_c(&mut self) -> CtrlCAction {
        let now = Instant::now();
        
        // 检查是否是双击 (2 秒内)
        if let Some(last_time) = self.last_ctrl_c_time {
            if now.duration_since(last_time) < Duration::from_secs(2) {
                // 双击：强制退出
                self.force_quit_flag.store(true, Ordering::SeqCst);
                return CtrlCAction::ForceQuit;
            }
        }
        
        // 单击：软中断
        self.last_ctrl_c_time = Some(now);
        self.cancel_token.cancel();
        
        CtrlCAction::Interrupt
    }
}

// 全局 Ctrl+C 处理器设置
pub fn setup_ctrl_c_handler(force_quit_flag: Arc<AtomicBool>) -> anyhow::Result<()> {
    ctrlc::set_handler(move || {
        let now = Instant::now();
        
        if now.duration_since(last_ctrl_c_time.get()) < Duration::from_secs(2) {
            // 双击：强制退出
            force_quit_flag.store(true, Ordering::SeqCst);
            eprintln!("强制退出...");
            std::process::exit(1);
        } else {
            // 单击：中断
            last_ctrl_c_time.set(now);
            if let Some(cancellation) = current_cancellation_token.get() {
                cancellation.cancel();
            }
            eprintln!("操作已中断，再次按 Ctrl+C 强制退出");
        }
    })?;
    
    Ok(())
}
```

### 4.3 不同状态的 Ctrl+C 处理

| 当前状态 | Ctrl+C 处理 | 说明 |
|----------|-------------|------|
| `Idle` | 无操作 | 空闲状态，无需中断 |
| `Submitting` | 中断提交 | 停止正在处理的用户输入 |
| `Waiting` | 中断等待 | 取消等待 AI 响应 |
| `Streaming` | 中断流式输出 | 停止 AI 输出，显示已生成内容 |
| `WaitingApproval` | 取消审批弹窗 | pop ApprovalOverlay，拒绝 AI 请求 |
| `Executing` | 中断执行 | 停止正在执行的命令或文件操作 |
| `Interrupted` | 强制退出 | 已中断状态，再次 Ctrl+C 强制退出 |
| `Error` | 清除错误 | 清除错误状态，回到 Idle |

### 4.4 中断恢复机制

```
中断后的恢复流程：
  1. 用户中断操作后，系统状态变为 Interrupted
  2. 等待用户确认操作:
     - 输入新内容 → 自动恢复到 Idle
     - 按 Esc → 回到 Idle
     - 等待 5 秒 → 自动恢复到 Idle
  3. 清理中断状态:
     - 重置 cancel_token
     - 清理部分结果
     - 恢复 UI 状态
  4. 显示恢复提示: "已就绪，可继续对话"
```

### 4.5 force_quit 通知机制

```rust
pub struct ForceQuitNotifier {
    force_quit_flag: Arc<AtomicBool>,
    notify_tx: mpsc::Sender<()>,
}

impl ForceQuitNotifier {
    pub async fn monitor_force_quit(&self) {
        while !self.force_quit_flag.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        // 发送强制退出通知
        let _ = self.notify_tx.send(()).await;
    }
}

// 在主循环中使用
tokio::select! {
    // ... 其他事件处理
    Some(()) = mut force_quit_notifier.rx.recv() => {
        // 处理强制退出
        break;
    }
}
```

---

## 5. 会话管理

### 5.1 会话 ID 生成

```rust
use uuid::Uuid;

pub fn generate_session_id() -> String {
    // 生成基于时间戳和随机数的唯一会话 ID
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let random_part = Uuid::new_v4().to_string()[..8].to_string();
    
    format!("loom_session_{timestamp}_{random_part}")
}
```

### 5.2 会话恢复机制

```rust
pub struct ResumePicker {
    saved_sessions: Vec<SessionMetadata>,
}

#[derive(Debug, Clone)]
pub struct SessionMetadata {
    pub session_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_active: chrono::DateTime<chrono::Utc>,
    pub message_count: usize,
    pub workspace: String,
    pub model: String,
}

impl ResumePicker {
    pub async fn load_sessions(&mut self) -> anyhow::Result<()> {
        // 从 ~/.loom/sessions/ 目录加载会话元数据
        let sessions_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("无法找到 home 目录"))?
            .join(".loom")
            .join("sessions");
        
        if !sessions_dir.exists() {
            return Ok(());
        }
        
        for entry in std::fs::read_dir(&sessions_dir)? {
            let entry = entry?;
            let metadata_path = entry.path().join("metadata.json");
            
            if metadata_path.exists() {
                let metadata: SessionMetadata = 
                    serde_json::from_str(&std::fs::read_to_string(&metadata_path)?)?;
                self.saved_sessions.push(metadata);
            }
        }
        
        // 按最后活跃时间排序
        self.saved_sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        
        Ok(())
    }
    
    pub fn select_session(&self, index: usize) -> Option<&SessionMetadata> {
        self.saved_sessions.get(index)
    }
}
```

### 5.3 线程日志管理

```rust
pub struct ThreadLog {
    session_id: String,
    events: Vec<ThreadEvent>,
    file_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThreadEvent {
    Started {
        timestamp: DateTime<Utc>,
        model: String,
        system_prompt: Option<String>,
    },
    UserMessage {
        timestamp: DateTime<Utc>,
        content: String,
    },
    AssistantMessage {
        timestamp: DateTime<Utc>,
        content: String,
        events: Vec<CodexEvent>,
    },
    ToolCall {
        timestamp: DateTime<Utc>,
        tool_name: String,
        args: serde_json::Value,
    },
    Error {
        timestamp: DateTime<Utc>,
        error: String,
    },
}

impl ThreadLog {
    pub fn new(session_id: String) -> anyhow::Result<Self> {
        let logs_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("无法找到 home 目录"))?
            .join(".loom")
            .join("logs");
        
        std::fs::create_dir_all(&logs_dir)?;
        
        let file_name = format!("{}.jsonl", session_id);
        let file_path = logs_dir.join(file_name);
        
        Ok(Self {
            session_id,
            events: Vec::new(),
            file_path,
        })
    }
    
    pub fn log_event(&mut self, event: ThreadEvent) -> anyhow::Result<()> {
        self.events.push(event.clone());
        
        // 追加到文件
        let log_entry = serde_json::to_string(&event)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;
        
        writeln!(file, "{}", log_entry)?;
        
        Ok(())
    }
    
    pub async fn save_session_metadata(&self, metadata: &SessionMetadata) -> anyhow::Result<()> {
        let sessions_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("无法找到 home 目录"))?
            .join(".loom")
            .join("sessions");
        
        std::fs::create_dir_all(&sessions_dir)?;
        
        let session_dir = sessions_dir.join(&self.session_id);
        std::fs::create_dir_all(&session_dir)?;
        
        let metadata_path = session_dir.join("metadata.json");
        std::fs::write(&metadata_path, serde_json::to_string_pretty(metadata)?)?;
        
        Ok(())
    }
}
```

### 5.4 会话恢复工作流

```
启动 Loom TUI
    │
    ▼
检查是否存在恢复会话
    │
    ├── 无会话 → 创建新会话
    │         │
    │         ▼
    │     generate_session_id()
    │     初始化新的 ThreadLog
    │
    └── 有会话 → 显示恢复选择器
              │
              └── ResumePicker::show()
                  │
                  ├── 用户选择会话 → 恢复会话
                  │                 │
                  │                 ▼
                  │             加载会话数据
                  │             恢复 ThreadLog
                  │             恢复对话历史
                  │             继续对话
                  │
                  ├── 用户选择新会话 → 创建新会话
                  │
                  └── 用户取消 → 退出
```

---

## 6. 对抗性验证

### 6.1 边缘情况处理

#### 通知系统边缘情况

| 边缘情况 | 问题描述 | 解决方案 |
|----------|----------|----------|
| 通知后端不可用 | 系统通知中心被禁用或失败 | 自动降级到 stderr 输出 |
| 暂停时收到通知 | ^Z 暂停后系统继续发送通知 | 通知入队，恢复后处理 |
| 通知风暴 | 短时间内大量事件触发通知 | 频率限制 + 批量合并 |
| 网络通知失败 | 远程通知服务不可达 | 本地缓存 + 重试机制 |

#### 进程管理边缘情况

| 边缘情况 | 问题描述 | 解决方案 |
|----------|----------|----------|
| 暂停后无法恢复 | ^Z 暂停后 fg 命令无效 | 使用 SIGCONT 直接恢复 |
| 外部程序崩溃 | 外部程序异常退出 | 捕获信号，清理状态后恢复 |
| 暂停时外部程序运行 | 暂停后外部程序继续运行 | 等待外部程序完成再暂停 |
| Resize 时事件丢失 | 快速 resize 导致事件丢失 | 定期轮询检查终端尺寸 |

### 6.2 失败模式分析

#### 通知系统失败模式

```
故障类型: 通知风暴
  触发条件: 大量错误事件或子代理频繁完成
  影响: 用户被大量通知打扰
  检测: 同类型通知频率 > 10次/分钟
  处理: 
    1. 自动启用批量模式
    2. 合并相似通知
    3. 发送摘要通知而非单独通知

故障类型: 通知后端完全失效
  触发条件: 平台通知服务停止或配置错误
  影响: 用户错过重要事件
  检测: 连续 10 次通知失败
  处理:
    1. 禁用该通知后端
    2. 降级到终端内状态提示
    3. 记录错误日志
    4. 定期重试恢复后端
```

#### 进程管理失败模式

```
故障类型: 暂停后无法恢复
  触发条件: 终端状态损坏或信号处理异常
  影响: 用户会话永久挂起
  检测: 30 秒内未收到恢复信号
  处理:
    1. 自动重试恢复 (最多 3 次)
    2. 重置终端状态
    3. 重新初始化 TUI
    4. 如仍失败，提供手动恢复指南

故障类型: Resize 死循环
  触发条件: resize 处理本身触发新的 resize 事件
  影响: CPU 占用 100%，界面冻结
  检测: 连续 5 次 resize 事件
  处理:
    1. 启用防抖 (debounce) 机制
    2. 忽略短时间内的后续 resize 事件
    3. 只处理最后一次 resize
```

### 6.3 安全考量

#### 通知系统安全

```
安全风险 1: 通知内容泄露
  风险: 通知可能包含敏感信息（文件内容、命令参数）
  缓解:
    - 通知内容脱敏处理
    - 只显示摘要信息
    - 用户可选择通知详细程度
    
安全风险 2: 通知服务权限提升
  风险: 恶意通知服务可能获得系统权限
  缓解:
    - 验证通知服务来源
    - 使用最小权限原则
    - 沙箱化通知处理
```

#### 进程管理安全

```
安全风险 1: 外部程序权限提升
  风险: 恶意外部程序可能获得 TUI 进程权限
  缓解:
    - 恢复终端前验证外部程序身份
    - 使用临时用户权限运行外部程序
    - 清理环境变量
    
安全风险 2: 信号处理劫持
  风险: 恶意代码可能劫持 ^Z/恢复信号
  缓解:
    - 信号处理器设置后锁定
    - 验证信号来源
    - 记录所有信号操作
```

### 6.4 设计权衡

#### 通知系统设计权衡

| 设计选择 | 优点 | 缺点 | 选择 |
|----------|------|------|------|
| 同步通知 vs 异步通知 | 同步：简单，保证送达；异步：不阻塞 | 异步：可能丢失事件；同步：影响性能 | 异步带重试 |
| 平台特定实现 vs 跨平台抽象 | 平台特定：性能好；跨平台：一致性好 | 平台特定：维护成本高；跨平台：功能受限 | 跨平台抽象 + 平台优化 |
| 通知聚合 vs 单独通知 | 聚合：减少打扰；单独：信息及时 | 聚合：信息延迟；单独：可能过多 | 混合模式 |

#### 进程管理设计权衡

| 设计选择 | 优点 | 缺点 | 选择 |
|----------|------|------|------|
| 完整暂停 vs 最小暂停 | 完整：状态一致；最小：恢复快速 | 完整：复杂；最小：可能丢失状态 | 完整暂停 |
| 自动恢复 vs 手动恢复 | 自动：用户体验好；手动：可控性强 | 自动：可能意外恢复；手动：需要用户操作 | 超时自动恢复 |
| Resize 同步处理 vs 异步处理 | 同步：状态一致；异步：性能好 | 同步：可能阻塞；异步：状态复杂 | 异步防抖 |

---

## 7. 总结

Loom TUI 的系统级交互功能通过以下设计原则确保稳定可靠的用户体验：

1. **状态一致性**: 暂停、恢复、resize 等操作都保证状态的完整恢复
2. **用户可控性**: 双击退出、审批机制、中断系统确保用户始终掌控
3. **渐进增强**: 通知系统支持降级，失败时有备用方案
4. **容错设计**: 对抗性验证覆盖边缘情况和失败模式
5. **性能优化**: 频率控制、防抖机制、异步处理确保性能

这些系统级功能与交互层的协同工作，为用户提供了完整的 TUI 体验，既保持了终端的原生特性，又增加了智能化的交互能力。