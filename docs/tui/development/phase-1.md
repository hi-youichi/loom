# Phase 1: 基础设施实现方案

## 概述

Phase 1 是 Loom TUI 的**基础设施层**，建立 TUI 基础运行环境。本阶段**不依赖 ratatui**，仅使用 crossterm 实现终端控制和事件捕获。

**目标**：在终端中显示一个固定区域的聊天视图，可接收键盘事件，退出时终端状态完全恢复。

**预计文件**：6 个文件，~800 行代码

---

## 1. 文件清单

| 文件 | 职责 | 预计行数 | 依赖 |
|------|------|----------|------|
| `tui/mod.rs` | 模块入口 + 条件编译 | ~30 | 无 |
| `tui/terminal.rs` | 终端初始化/恢复 + 内联视图 | ~200 | crossterm |
| `tui/event.rs` | 事件系统 (TuiEvent + EventBroker) | ~150 | crossterm, tokio |
| `tui/viewport.rs` | Viewport 位置管理 | ~120 | crossterm |
| `tui/history.rs` | 历史行插入 | ~100 | crossterm |
| `tui/app.rs` | App 主循环骨架 | ~200 | 以上所有 |

---

## 2. 核心实现

### 2.1 终端初始化 (`tui/terminal.rs`)

#### 2.1.1 初始化流程

```rust
pub fn init() -> Result<InitializedTerminal> {
    // 1. 启用 raw mode（逐字符输入）
    enable_raw_mode()?;
    
    // 2. 启用 bracketed paste（区分粘贴和手动输入）
    execute!(stdout(), EnableBracketedPaste)?;
    
    // 3. 启用焦点变化事件（Unix）
    execute!(stdout(), EnableFocusChange)?;
    
    // 4. 设置 panic hook（panic 时恢复终端）
    set_panic_hook();
    
    // 5. 清除启动时缓冲的按键
    flush_terminal_input_buffer()?;
    
    // 6. 探测光标位置（决定 viewport 起始位置）
    let cursor_pos = probe_cursor_position()?;
    
    Ok(InitializedTerminal {
        cursor_pos,
        // ...
    })
}
```

#### 2.1.2 恢复流程

```rust
pub fn restore() -> Result<()> {
    // 1. 禁用 bracketed paste
    execute!(stdout(), DisableBracketedPaste)?;
    
    // 2. 禁用焦点变化
    execute!(stdout(), DisableFocusChange)?;
    
    // 3. 禁用 raw mode
    disable_raw_mode()?;
    
    // 4. 显示光标
    execute!(stdout(), Show)?;
    
    Ok(())
}
```

#### 2.1.3 Panic Hook

```rust
fn set_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // 恢复终端状态
        let _ = restore();
        // 调用原始 hook
        prev(panic_info);
    }));
}
```

#### 2.1.4 光标探测

```rust
/// 探测当前光标位置，用于确定 viewport 起始位置
fn probe_cursor_position() -> Result<(u16, u16)> {
    // 发送 DSR 请求
    execute!(stdout(), RequestPosition)?;
    // 读取响应 (CSI R)
    // 返回 (row, col)
}
```

### 2.2 事件系统 (`tui/event.rs`)

#### 2.2.1 TuiEvent 枚举

```rust
#[derive(Debug, Clone)]
pub enum TuiEvent {
    /// 按键事件（已处理 focus/paste 协议层）
    Key(KeyEvent),
    /// 粘贴内容
    Paste(String),
    /// 终端尺寸变化
    Resize(Size),
    /// 定时重绘信号
    Draw,
    /// 从 ^Z 暂停恢复
    Resume,
}
```

#### 2.2.2 EventBroker

```rust
/// Arc 共享的事件分发器，支持 pause/resume
#[derive(Clone)]
pub struct EventBroker {
    paused: Arc<AtomicBool>,
    tx: mpsc::UnboundedSender<TuiEvent>,
    rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<TuiEvent>>>>,
}

impl EventBroker {
    pub fn new() -> (Self, impl Stream<Item = TuiEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let broker = Self {
            paused: Arc::new(AtomicBool::new(false)),
            tx,
            rx: Arc::new(Mutex::new(Some(rx))),
        };
        let stream = ReceiverStream::new(broker.rx.lock().unwrap().take().unwrap());
        (broker, stream)
    }
    
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }
    
    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
    }
    
    pub fn send(&self, event: TuiEvent) {
        if !self.paused.load(Ordering::Relaxed) {
            let _ = self.tx.send(event);
        }
    }
}
```

#### 2.2.3 事件流包装

```rust
/// 包装 crossterm EventStream，生成 TuiEvent 流
pub fn event_stream(
    broker: EventBroker,
) -> impl Stream<Item = TuiEvent> {
    let crossterm_stream = crossterm::event::EventStream::new();
    
    // 定时器：每 100ms 发送 Draw 事件
    let draw_tick = tokio::time::interval(Duration::from_millis(100));
    
    stream! {
        let mut crossterm_stream = crossterm_stream;
        let mut draw_tick = draw_tick;
        
        loop {
            tokio::select! {
                Some(Ok(event)) = crossterm_stream.next() => {
                    match event {
                        Event::Key(key) => {
                            broker.send(TuiEvent::Key(key));
                        }
                        Event::Paste(text) => {
                            broker.send(TuiEvent::Paste(text));
                        }
                        Event::Resize(w, h) => {
                            broker.send(TuiEvent::Resize(Size::new(w, h)));
                        }
                        Event::FocusGained => {
                            // 更新 terminal_focused
                        }
                        Event::FocusLost => {
                            // 更新 terminal_focused
                        }
                        _ => {}
                    }
                }
                _ = draw_tick.tick() => {
                    broker.send(TuiEvent::Draw);
                }
            }
        }
    }
}
```

### 2.3 Viewport 管理 (`tui/viewport.rs`)

#### 2.3.1 Viewport 结构体

```rust
/// 管理内联视图的屏幕位置
pub struct Viewport {
    /// Viewport 在屏幕中的起始行（0-based）
    top: u16,
    /// Viewport 高度
    height: u16,
    /// Viewport 宽度（等于终端宽度）
    width: u16,
    /// 屏幕总高度
    screen_height: u16,
    /// 是否底部对齐
    bottom_aligned: bool,
}
```

#### 2.3.2 核心方法

```rust
impl Viewport {
    /// 创建新的 viewport，初始位置在光标下方
    pub fn new(cursor_row: u16, screen_size: Size) -> Self {
        Self {
            top: cursor_row.saturating_add(1), // 光标下一行
            height: 10, // 初始高度
            width: screen_size.width,
            screen_height: screen_size.height,
            bottom_aligned: false,
        }
    }
    
    /// 获取 viewport 的矩形区域
    pub fn rect(&self) -> Rect {
        Rect::new(0, self.top, self.width, self.height)
    }
    
    /// 终端 resize 时更新 viewport 位置
    pub fn handle_resize(&mut self, new_size: Size) -> bool {
        let old_height = self.screen_height;
        self.screen_height = new_size.height;
        self.width = new_size.width;
        
        if self.bottom_aligned {
            // 底部对齐：调整 top 保底底部位置不变
            let bottom = self.top.saturating_add(self.height);
            if bottom > self.screen_height {
                self.top = self.screen_height.saturating_sub(self.height);
            }
        }
        
        // 返回是否需要全量重绘
        old_height != self.screen_height
    }
    
    /// 设置底部对齐
    pub fn set_bottom_aligned(&mut self, aligned: bool) {
        self.bottom_aligned = aligned;
        if aligned {
            self.top = self.screen_height.saturating_sub(self.height);
        }
    }
}
```

### 2.4 历史行插入 (`tui/history.rs`)

#### 2.4.1 历史行缓冲

```rust
/// 待写入终端滚动区域的历史行
pub struct PendingHistory {
    lines: Vec<(String, HistoryLineWrapPolicy)>,
}

pub enum HistoryLineWrapPolicy {
    /// 预先换行（手动控制换行位置）
    PreWrap,
    /// 让终端处理换行
    Terminal,
}

impl PendingHistory {
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }
    
    /// 添加一行到待处理队列
    pub fn push(&mut self, line: String, wrap: HistoryLineWrapPolicy) {
        self.lines.push((line, wrap));
    }
    
    /// 获取待处理行数
    pub fn len(&self) -> usize {
        self.lines.len()
    }
    
    /// 是否有待处理行
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}
```

#### 2.4.2 Flush 实现

```rust
impl PendingHistory {
    /// 将所有待处理行写入终端（在 draw 时调用）
    pub fn flush(&mut self) -> Result<()> {
        if self.lines.is_empty() {
            return Ok(());
        }
        
        // 使用 SynchronizedUpdate 包裹，避免闪烁
        stdout().sync_update(|_| {
            for (line, _wrap) in &self.lines {
                // 移动到 viewport 上方的行
                // 写入历史行
                write!(stdout(), "{}\r\n", line)?;
            }
            stdout().flush()?;
            Ok(())
        })?;
        
        self.lines.clear();
        Ok(())
    }
}
```

### 2.5 App 主循环 (`tui/app.rs`)

#### 2.5.1 App 结构体

```rust
/// TUI 应用主循环
pub struct App {
    /// 终端管理器
    terminal: TerminalManager,
    /// 事件流
    event_stream: Pin<Box<dyn Stream<Item = TuiEvent> + Send>>,
    /// 事件代理
    event_broker: EventBroker,
    /// Viewport 管理器
    viewport: Viewport,
    /// 待处理历史行
    pending_history: PendingHistory,
    /// 应用状态
    state: AppState,
    /// Agent 事件接收器
    agent_rx: Option<mpsc::Receiver<AgentEvent>>,
}
```

#### 2.5.2 主循环

```rust
impl App {
    pub async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Some(event) = self.event_stream.next() => {
                    match event {
                        TuiEvent::Key(key) => self.handle_key(key),
                        TuiEvent::Resize(size) => self.handle_resize(size),
                        TuiEvent::Draw => self.render(),
                        TuiEvent::Resume => self.handle_resume(),
                        TuiEvent::Paste(text) => self.handle_paste(text),
                    }
                }
                Some(event) = self.agent_rx.as_mut().unwrap().recv() => {
                    self.handle_agent_event(event);
                }
            }
        }
    }
    
    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+C：中断当前操作
                self.handle_ctrl_c();
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+D：退出
                self.handle_ctrl_d();
            }
            KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+Z：暂停（Unix）
                self.handle_ctrl_z();
            }
            _ => {
                // 转发到事件分发器
                self.dispatch_key(key);
            }
        }
    }
    
    fn render(&mut self) {
        // 1. flush 待处理历史行
        self.pending_history.flush().ok();
        
        // 2. 计算 viewport 区域
        let area = self.viewport.rect();
        
        // 3. 渲染到终端
        // Phase 1: 简单文本渲染
        // Phase 2+: 使用 ratatui
        self.render_text(area);
    }
}
```

---

## 3. 集成点

### 3.1 main.rs 集成

```rust
// main.rs
#[cfg(feature = "tui")]
mod tui;

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
            eprintln!("TUI mode is not enabled. Rebuild with --features tui");
            return Ok(());
        }
    }
    
    // 传统模式
    run_traditional().await
}
```

### 3.2 Cargo.toml 变更

```toml
[features]
default = []
tui = ["ratatui", "crossterm", "crossterm/event-stream"]

[dependencies]
crossterm = { version = "0.28", optional = true, features = ["event-stream", "bracketed-paste"] }
```

---

## 4. 关键测试

### 4.1 单元测试

| 测试 | 文件 | 测试内容 |
|------|------|----------|
| `test_init_restore` | `terminal.rs` | 初始化后终端进入 raw mode，恢复后退出 raw mode |
| `test_event_broker_pause_resume` | `event.rs` | 暂停后事件不发送，恢复后继续发送 |
| `test_viewport_resize` | `viewport.rs` | 终端缩小/放大时 viewport 位置正确 |
| `test_viewport_bottom_aligned` | `viewport.rs` | 底部对齐模式正确 |
| `test_pending_history_flush` | `history.rs` | 历史行正确缓冲和 flush |
| `test_app_ctrl_c` | `app.rs` | Ctrl+C 正确中断操作 |
| `test_app_ctrl_d` | `app.rs` | Ctrl+D 正确退出 |

### 4.2 手动验证

| 验证项 | 操作 | 预期结果 |
|--------|------|----------|
| 终端初始化 | 运行 `cargo run -- -i` | 终端进入 TUI 模式，显示聊天区域 |
| 按键捕获 | 按任意键 | 事件被正确捕获和打印 |
| Resize 处理 | 调整终端窗口大小 | Viewport 正确调整位置 |
| Ctrl+C 中断 | 按 Ctrl+C | 中断当前操作，不会退出 |
| Ctrl+D 退出 | 按 Ctrl+D | 退出 TUI 模式，终端恢复 |
| 退出恢复 | 正常退出 | 终端完全恢复到初始状态 |

---

## 5. 实现顺序

### Step 1: 模块入口 + 条件编译

1. 创建 `apps/cli/src/tui/mod.rs`
2. 添加 `#[cfg(feature = "tui")]` 编译守卫
3. 暴露公共类型

### Step 2: 终端初始化

1. 实现 `init()` 和 `restore()`
2. 实现 `set_panic_hook()`
3. 实现 `flush_terminal_input_buffer()`
4. 实现 `probe_cursor_position()`

### Step 3: 事件系统

1. 实现 `TuiEvent` 枚举
2. 实现 `EventBroker`（pause/resume/send）
3. 实现 `event_stream()` 包装函数

### Step 4: Viewport 管理

1. 实现 `Viewport` 结构体
2. 实现 `new()`、`rect()`、`handle_resize()`
3. 实现 `set_bottom_aligned()`

### Step 5: 历史行插入

1. 实现 `PendingHistory` 结构体
2. 实现 `push()` 和 `flush()`

### Step 6: App 主循环

1. 实现 `App` 结构体
2. 实现 `run()` 主循环
3. 实现基本按键处理（Ctrl+C, Ctrl+D, Ctrl+Z）
4. 实现简单渲染（文本输出）

---

## 6. 代码示例

### 6.1 mod.rs

```rust
//! Loom TUI - Interactive Terminal User Interface
//!
//! Provides an optional interactive TUI mode for Loom CLI.
//! Activated via `--interactive` / `-i` flag.

pub mod app;
pub mod event;
pub mod history;
pub mod terminal;
pub mod viewport;

pub use app::App;
pub use event::{EventBroker, TuiEvent};
pub use terminal::{init, restore};
```

### 6.2 terminal.rs 骨架

```rust
use std::io::stdout;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    event::{DisableBracketedPaste, EnableBracketedPaste, DisableFocusChange, EnableFocusChange},
    cursor::{Show, Hide},
};

/// 初始化终端进入 TUI 模式
pub fn init() -> Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnableBracketedPaste)?;
    execute!(stdout(), EnableFocusChange)?;
    execute!(stdout(), Hide)?;
    set_panic_hook();
    Ok(())
}

/// 恢复终端到初始状态
pub fn restore() -> Result<()> {
    execute!(stdout(), DisableBracketedPaste)?;
    execute!(stdout(), DisableFocusChange)?;
    execute!(stdout(), Show)?;
    disable_raw_mode()?;
    Ok(())
}

fn set_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore();
        prev(panic_info);
    }));
}
```

### 6.3 app.rs 骨架

```rust
use tokio_stream::Stream;
use std::pin::Pin;

use super::event::{EventBroker, TuiEvent};
use super::viewport::Viewport;

pub struct App {
    event_broker: EventBroker,
    event_stream: Pin<Box<dyn Stream<Item = TuiEvent> + Send>>,
    viewport: Viewport,
    running: bool,
}

impl App {
    pub fn new() -> Result<Self> {
        let (broker, stream) = EventBroker::new();
        
        Ok(Self {
            event_broker: broker,
            event_stream: Box::pin(stream),
            viewport: Viewport::new(0, Size::new(80, 24)),
            running: true,
        })
    }
    
    pub async fn run(&mut self) -> Result<()> {
        super::terminal::init()?;
        
        while self.running {
            tokio::select! {
                Some(event) = self.event_stream.next() => {
                    self.handle_event(event);
                }
            }
        }
        
        super::terminal::restore()?;
        Ok(())
    }
    
    fn handle_event(&mut self, event: TuiEvent) {
        match event {
            TuiEvent::Key(key) => {
                // Phase 1: 简单处理
                if key.code == KeyCode::Char('d') 
                    && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.running = false;
                }
            }
            TuiEvent::Resize(size) => {
                self.viewport.handle_resize(size);
            }
            TuiEvent::Draw => {
                // Phase 1: 简单文本渲染
            }
            _ => {}
        }
    }
}
```

---

## 7. 交付标准

- [x] `tui/mod.rs` 模块入口，条件编译支持
- [x] `tui/terminal.rs` 终端初始化/恢复，panic hook
- [x] `tui/event.rs` 事件系统（TuiEvent + EventBroker）
- [x] `tui/viewport.rs` Viewport 管理
- [x] `tui/history.rs` 历史行插入
- [x] `tui/app.rs` App 主循环
- [x] `Cargo.toml` 依赖配置
- [x] `main.rs` `--interactive` 参数处理
- [ ] 单元测试覆盖
- [ ] 手动验证通过

---

## 8. 注意事项

1. **crossterm 版本兼容性**：0.28 版本的 `event-stream` feature 需要 `tokio-stream` 配合
2. **光标探测**：`probe_cursor_position()` 在部分终端可能不准确，需要 fallback
3. **Panic 安全**：`set_panic_hook()` 必须确保在任何 panic 情况下终端都能恢复
4. **Windows 兼容**：Phase 1 优先支持 Unix，Windows 延后处理
5. **Zellij 兼容**：Zellij 的历史行插入需要特殊处理（`InsertHistoryMode::ZellijRaw`），Phase 1 不实现