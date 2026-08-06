# Loom TUI 事件系统与输入处理

## 概述

本文档详细描述 Loom TUI 的事件系统与输入处理机制，包括事件类型定义、事件路由架构、按键映射系统，以及对抗性验证分析。所有设计均基于 Codex CLI 源码推导和 Loom 架构设计验证。

**核心价值**：提供可预测、响应迅速、安全可控的用户交互体验，确保每个用户输入都能被正确路由和处理。

---

## 1. 事件类型定义

### 1.1 TuiEvent 枚举（推导自 `event_stream.rs`）

```rust
/// Loom TUI 统一事件类型
#[derive(Debug, Clone, PartialEq)]
pub enum TuiEvent {
    /// 键盘事件 - 来自 crossterm::event::KeyEvent
    Key(KeyEvent),
    
    /// 粘贴事件 - 来自 bracketed paste 模式解析
    Paste(String),
    
    /// 终端尺寸变化 - 来自 crossterm::event::Resize
    Resize { columns: u16, rows: u16 },
    
    /// 焦点事件 - 终端获得/失去焦点
    FocusGained,
    FocusLost,
    
    /// 渲染触发 - 来自定时器或事件请求
    Draw,
    
    /// 暂停恢复 - 来自 job control 的 SIGCONT
    Resume,
    
    /// 定时器事件 - 用于超时和节流
    Timer {
        id: String,
        payload: Option<serde_json::Value>,
    },
}

/// 键盘事件封装
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub kind: KeyEventKind,
}

pub enum KeyCode {
    Char(char),
    Up, Down, Left, Right,
    Enter, Tab, Backspace, Esc,
    F(u8),
    Null,
}

pub struct KeyModifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
}
```

### 1.2 事件来源映射

> **注意**：以下源码引用为基于 Codex CLI 架构的推导（`reference-codex.md`），Loom TUI 当前尚未实现独立的事件流模块。所有事件类型均通过 crossterm 捕获，在 App 主循环中统一处理。

| TuiEvent 变体 | crossterm 原始来源 | 说明 |
|--------------|------------------|------|
| `TuiEvent::Key` | `crossterm::event::Event::Key` | 键盘事件，由 crossterm 原始捕获 |
| `TuiEvent::Paste` | `crossterm::event::Event::Paste` |纠结粘贴事件，由 crossterm 提供 bracketed paste 支持 |
| `TuiEvent::Resize` | `crossterm::event::Event::Resize` | 终端尺寸变化 |
| `TuiEvent::FocusGained` | `crossterm::event::Event::FocusGained` | 终端获得焦点 |
| `TuiEvent::FocusLost` | `crossterm::event::Event::FocusLost` | 终端失去焦点 |
| `TuiEvent::Draw` | `tokio::time::interval` 定时器 | 渲染帧触发 |
| `TuiEvent::Resume` | SIGCONT 信号处理 | 从暂停恢复 |

### 1.3 Codex vs Loom 事件类型对比

| 维度 | Codex CLI | Loom TUI |
|------|-----------|----------|
| **事件架构** | JSON-RPC 事件流（`event_bridge.rs`） | 原生终端事件流（`TuiEventStream`） |
| **键盘事件** | 通过 `stdio_loop.rs` 作为 JSON 消息 | 直接来自 crossterm `EventStream` |
| **事件类型** | `CodexEvent` 枚举（业务事件） | `TuiEvent` 枚举（UI 事件） |
| **事件传输** | JSON 序列化 over stdin/stdout | 内存中 `EventBroker` 分发 |
| **焦点事件** | 无明确焦点事件 | 支持 `FocusGained`/`FocusLost` |
| **定时器** | Agent 内部流式事件 | 独立的 `Timer` 事件类型 |

**Codex 事件特点**（实际定义在 `foundation/stream-event/src/codex.rs:6-15`）：
```rust
// Codex 使用 JSON-RPC 协议，事件通过 stdio 传输
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexEvent {
    ThreadStarted { thread_id: String },
    TurnStarted,
    TurnCompleted { usage: CodexUsage },
    TurnFailed { error: CodexErrorInfo },
    ItemStarted { item: Value },
    ItemUpdated { item: Value },
    ItemCompleted { item: Value },
    Error { message: String },
}
```

**Loom 事件特点**：
- 直接捕获终端事件，减少序列化开销
- 区分 UI 事件和业务事件，职责分离
- 支持焦点事件和定时器，交互更丰富

---

## 2. 事件路由架构

### 2.1 事件流管道

```
┌─────────────────────────────────────────────────────────────────────┐
│                        事件处理管道                                 │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  1. 事件捕获层 (Event Capture)                                │  │
│  │  ┌────────────────────────────────────────────────────────┐  │  │
│  │  │ crossterm::EventStream                                 │  │  │
│  │  │   │                                                     │  │  │
│  │  │   ├── Event::Key(KeyEvent)                            │  │  │
│  │  │   ├── Event::Paste(String)                            │  │  │
│  │  │   ├── Event::Resize(width, height)                    │  │  │
│  │  │   ├── Event::FocusGained/FocusLost                    │  │  │
│  │  │   └── Event::Mouse(...)                               │  │  │
│  │  │                                                         │  │  │
│  │  │   ▼                                                     │  │  │
│  │  │ TuiEventStream (包装层)                                │  │  │
│  │  │   ├── 解析 bracketed paste                             │  │  │
│  │  │   ├── 检测焦点变化                                    │  │  │
│  │  │   ├── 合并重复 resize 事件                            │  │  │
│  │  │   └── 转换为 TuiEvent                                 │  │  │
│  │  └────────────────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                           │                                          │
│                           ▼                                          │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  2. 事件分发层 (Event Distribution)                           │  │
│  │  ┌────────────────────────────────────────────────────────┐  │  │
│  │  │ EventBroker (Arc 共享，支持 pause/resume)             │  │  │
│  │  │   │                                                     │  │  │
│  │  │   ├── 支持多消费者                                     │  │  │
│  │  │   ├── 事件队列 (mpsc channel)                         │  │  │
│  │  │   ├── pause/resume 控制流                              │  │  │
│  │  │   └── 事件优先级队列                                   │  │  │
│  │  └────────────────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                           │                                          │
│                           ▼                                          │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  3. 事件路由层 (Event Routing)                                │  │
│  │  ┌────────────────────────────────────────────────────────┐  │  │
│  │  │ 事件优先级判断:                                        │  │  │
│  │  │   ├─ Ctrl+Z → SuspendContext (最高)                   │  │  │
│  │  │   ├─ Ctrl+C → interrupt logic                        │  │  │
│  │  │   ├─ Resize → viewport adjustment                    │  │  │
│  │  │   ├─ Key events → PaneStack routing                  │  │  │
│  │  │   ├─ Paste → ChatComposer                            │  │  │
│  │  │   └─ Draw → rendering pipeline                        │  │  │
│  │  └────────────────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                           │                                          │
│                           ▼                                          │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  4. 事件处理层 (Event Handling)                               │  │
│  │  ┌────────────────────────────────────────────────────────┐  │  │
│  │  │ App::handle_event(TuiEvent) → Handled/NotHandled      │  │  │
│  │  │   │                                                     │  │  │
│  │  │   ├── handle_key_event() → PaneStack::handle_key()     │  │  │
│  │  │   ├── handle_paste_event() → ChatComposer::paste()     │  │  │
│  │  │   ├── handle_resize_event() → viewport_recalculate()   │  │  │
│  │  │   ├── handle_draw_event() → render()                   │  │  │
│  │  │   └── handle_resume_event() → restore_terminal()       │  │  │
│  │  └────────────────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                           │                                          │
│                           ▼                                          │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  5. 视图响应层 (View Response)                                │  │
│  │  ┌────────────────────────────────────────────────────────┐  │  │
│  │  │ PaneView::handle_key_event(key) → Handled/NotHandled │  │  │
│  │  │   │                                                     │  │  │
│  │  │   ├── ChatComposer: 编辑、历史、提交                   │  │  │
│  │  │   ├── ApprovalOverlay: Y/N/D/A                        │  │  │
│  │  │   ├── ListSelectionView: ↑/↓/Enter/搜索               │  │  │
│  │  │   └── 未处理事件传递给下一层                           │  │  │
│  │  └────────────────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.2 事件优先级系统

```rust
/// 事件优先级枚举（从 architecture.md 推导）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventPriority {
    Critical,   // Ctrl+Z: 暂停优先级
    High,       // Ctrl+C: 中断优先级
    Important,  // Resize: 布局调整优先级
    Normal,     // 键盘/Paste: 正常交互
    Low,        // Draw: 渲染优先级
}

/// 事件优先级映射（推导自 architecture.md:152-159）
fn event_priority(event: &TuiEvent) -> EventPriority {
    match event {
        TuiEvent::Key(KeyEvent { code: KeyCode::Char('z'), modifiers: KeyModifiers { control: true, .. }, .. }) 
            => EventPriority::Critical,
        
        TuiEvent::Key(KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers { control: true, .. }, .. }) 
            => EventPriority::High,
        
        TuiEvent::Resize { .. } 
            => EventPriority::Important,
        
        TuiEvent::Key(_) | TuiEvent::Paste(_) 
            => EventPriority::Normal,
        
        TuiEvent::Draw 
            => EventPriority::Low,
        
        _ => EventPriority::Normal,
    }
}
```

### 2.3 事件处理契约

```rust
/// 事件处理结果（推导自 architecture.md:165-183）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handled {
    /// 事件已处理，无需继续传递
    Handled,
    /// 事件未处理，传递给下一层
    NotHandled,
}

/// Ctrl+C 处理结果（参考 architecture.md 定义）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtrlCAction {
    /// 未处理，执行默认中断行为
    NotHandled,
    /// 已处理，无需中断
    Handled,
    /// 确认中断
    Cancel,
}
```

### 2.4 事件路由算法

```rust
/// 事件路由核心逻辑（推导自 architecture.md:228-242）
impl App {
    pub fn route_event(&mut self, event: TuiEvent) -> Handled {
        // 1. 检查优先级事件
        if let TuiEvent::Key(key) = &event {
            if self.handle_priority_keys(key) {
                return Handled::Handled;
            }
        }
        
        // 2. 根据 TuiEvent 类型路由
        match event {
            TuiEvent::Key(key) => self.handle_key_event(key),
            TuiEvent::Paste(text) => self.handle_paste_event(text),
            TuiEvent::Resize { columns, rows } => self.handle_resize_event(columns, rows),
            TuiEvent::FocusGained | TuiEvent::FocusLost => self.handle_focus_event(event),
            TuiEvent::Draw => self.handle_draw_event(),
            TuiEvent::Resume => self.handle_resume_event(),
            TuiEvent::Timer { id, payload } => self.handle_timer_event(id, payload),
        }
    }
    
    fn handle_priority_keys(&mut self, key: &KeyEvent) -> bool {
        // Ctrl+Z 优先处理
        if key.code == KeyCode::Char('z') && key.modifiers.control {
            self.suspend_context.prepare_suspend_action();
            return true;
        }
        
        // Ctrl+C 中断处理
        if key.code == KeyCode::Char('c') && key.modifiers.control {
            if let Some(ctrl_c_action) = self.pane_stack.handle_ctrl_c() {
                return ctrl_c_action == CtrlCAction::Handled;
            }
            self.handle_interrupt();
            return true;
        }
        
        false
    }
}
```

### 2.5 Codex vs Loom 事件路由对比

| 维度 | Codex CLI | Loom TUI |
|------|-----------|----------|
| **路由方式** | JSON-RPC 方法分发（`agent.rs:80-99`） | 事件优先级 + PaneStack 委托 |
| **中断机制** | `turn/cancel` JSON-RPC 方法 | 原生 Ctrl+C 事件处理 |
| **暂停机制** | 无明确机制 | `SuspendContext` + SIGCONT |
| **事件传递** | JSON 消息顺序传递 | 内存中优先级队列 |
| **并行处理** | `tokio::spawn` 异步处理（`stdio_loop.rs:55`） | 单线程事件循环 + 异步状态更新 |

**Codex 路由特点**（实际代码 `agent.rs:80-99`）：
```rust
// Codex 通过 JSON-RPC 方法路由
match method.as_str() {
    "thread/start" => self.handle_thread_start(id, params).await,
    "thread/resume" => self.handle_thread_resume(id, params).await,
    "turn/start" => self.handle_turn_start(id, params).await,
    "turn/cancel" => self.handle_turn_cancel(id, params).await,
    "thread/commandExecution/approve" => self.handle_command_approval(id, params, true).await,
    "thread/commandExecution/deny" => self.handle_command_approval(id, params, false).await,
    other => {
        tracing::warn!("Unknown method: {other}");
        // ...
    }
}
```

**Loom 路由优势**：
- 更低的事件处理延迟（无序列化开销）
- 支持事件优先级（确保关键操作响应）
- 原生终端事件支持（焦点、粘贴、resize）
- 线程安全的事件传递（`Arc<EventBroker>`）

---

## 3. 按键映射系统

### 3.1 KeymapRegistry 架构

```rust
/// 按键映射注册表（推导自 architecture.md:386-403）
pub struct KeymapRegistry {
    global_keymap: HashMap<KeyEvent, KeyAction>,
    view_keymaps: HashMap<ViewId, HashMap<KeyEvent, KeyAction>>,
    context_keymaps: HashMap<ContextId, HashMap<KeyEvent, KeyAction>>,
}

pub enum ViewId {
    Composer,
    Approval,
    Selection,
    Transcript,
}

pub enum ContextId {
    Normal,
    Searching,
    Editing,
    Approving,
}

/// 按键动作定义
pub enum KeyAction {
    Global(GlobalAction),
    ViewSpecific(ViewAction),
    Contextual(ContextAction),
}

pub enum GlobalAction {
    Interrupt,      // Ctrl+C
    Suspend,        // Ctrl+Z
    Exit,           // Ctrl+D
    ToggleTranscript, // Ctrl+T
}

pub enum ViewAction {
    Submit,         // Enter
    NavigateUp,     // ↑
    NavigateDown,   // ↓
    Cancel,         // Esc
    Accept,         // Y
    Reject,         // N
    Detail,         // D
    AlwaysAllow,    // A
}

pub enum ContextAction {
    SlashCommand,   // /
    SearchFilter,   // / in selection
    HistoryPrev,    // Ctrl+K
    HistoryNext,    // Ctrl+J
}
```

### 3.2 完整按键映射表

#### 3.2.1 全局按键（推导自 `reference-codex.md:185-197`）

| 按键 | 动作 | 优先级 | 处理位置 | 备注 |
|------|------|--------|----------|------|
| `Ctrl+C` | `Interrupt` | Critical | `App::handle_interrupt()` | 中断当前操作，支持确认流程 |
| `Ctrl+Z` | `Suspend` | Critical | `SuspendContext` | Unix 暂停，恢复 via `fg` |
| `Ctrl+D` | `Exit` | High | `App::handle_exit()` | 退出 Loom TUI |
| `Ctrl+T` | `ToggleTranscript` | High | `App::toggle_transcript()` | 切换转录覆盖层 |
| `Tab` | `SubmitOrQueue` | High | `ChatComposer::handle_tab()` | 提交或排队（基于任务状态） |
| `Ctrl+R` | `HistorySearch` | Normal | `ChatComposer::history_search()` | 反向搜索历史 |

#### 3.2.2 输入框按键（推导自 `reference-codex.md:200-212`）

| 按键 | 动作 | 优先级 | 处理位置 | 备注 |
|------|------|--------|----------|------|
| `Enter` | `Submit` | Normal | `ChatComposer::handle_enter()` | 提交当前输入 |
| `Shift+Enter` | `NewLine` | Normal | `ChatComposer::handle_shift_enter()` | 输入框内换行 |
| `↑/↓` | `HistoryNavigate` | Normal | `ChatComposer::history_navigate()` | 浏览输入历史 |
| `Ctrl+K` | `KillLineToEnd` | Normal | `ChatComposer::kill_to_end()` | 删除光标到行尾 |
| `Ctrl+U` | `KillLineToStart` | Normal | `ChatComposer::kill_to_start()` | 删除光标到行首 |
| `Ctrl+W` | `KillWordBackward` | Normal | `ChatComposer::kill_word_backward()` | 删除前一个词 |
| `Ctrl+A` | `MoveToLineStart` | Normal | `ChatComposer::move_to_start()` | 光标移到行首 |
| `Ctrl+E` | `MoveToLineEnd` | Normal | `ChatComposer::move_to_end()` | 光标移到行尾 |
| `Ctrl+←/→` | `MoveByWord` | Normal | `ChatComposer::move_by_word()` | 按词移动光标 |
| `/` | `SlashCommand` | Normal | `ChatComposer::handle_slash()` | 触发 slash 命令 |

#### 3.2.3 审批视图按键（推导自 `reference-codex.md:215-225`）

| 按键 | 动作 | 优先级 | 处理位置 | 备注 |
|------|------|--------|----------|------|
| `Y` / `Enter` | `Accept` | View | `ApprovalOverlay::handle_accept()` | 接受 AI 请求 |
| `N` | `Reject` | View | `ApprovalOverlay::handle_reject()` | 拒绝 AI 请求 |
| `D` | `Detail` | View | `ApprovalOverlay::handle_detail()` | 查看详细信息 |
| `A` | `AlwaysAllow` | View | `ApprovalOverlay::handle_always_allow()` | 本次会话始终允许 |
| `Esc` | `Cancel` | View | `ApprovalOverlay::handle_cancel()` | 取消审批 |
| `↑/↓` | `Navigate` | View | `ApprovalOverlay::handle_navigate()` | 在多文件间导航 |

#### 3.2.4 选择列表按键（推导自 `reference-codex.md:228-236`）

| 按键 | 动作 | 优先级 | 处理位置 | 备注 |
|------|------|--------|----------|------|
| `↑/↓` | `Navigate` | View | `ListSelectionView::handle_navigate()` | 浏览选项 |
| `Enter` | `Select` | View | `ListSelectionView::handle_select()` | 确认当前选项 |
| `Esc` | `Cancel` | View | `ListSelectionView::handle_cancel()` | 取消选择 |
| `/` | `SearchFilter` | Context | `ListSelectionView::handle_search()` | 进入搜索模式 |

#### 3.2.5 转录覆盖层按键（推导自 `reference-codex.md:239-247`）

| 按键 | 动作 | 优先级 | 处理位置 | 备注 |
|------|------|--------|----------|------|
| `↑/↓` | `Scroll` | View | `TranscriptOverlay::handle_scroll()` | 滚动查看 |
| `PgUp/PgDn` | `PageScroll` | View | `TranscriptOverlay::handle_page_scroll()` | 翻页查看 |
| `Enter` | `SelectAndInsert` | View | `TranscriptOverlay::handle_select_insert()` | 选择并插入到输入 |
| `Esc` | `Close` | View | `TranscriptOverlay::handle_close()` | 关闭覆盖层 |

### 3.3 按键映射优先级处理

```rust
/// 按键映射优先级解析（推导自 architecture.md:408-418）
impl KeymapRegistry {
    pub fn resolve_key_action(&self, key: &KeyEvent, current_view: Option<ViewId>, current_context: Option<ContextId>) -> Option<KeyAction> {
        // 1. 全局按键优先
        if let Some(action) = self.global_keymap.get(key) {
            return Some(action.clone());
        }
        
        // 2. 视图特定按键次之
        if let Some(view_id) = current_view {
            if let Some(view_keymap) = self.view_keymaps.get(&view_id) {
                if let Some(action) = view_keymap.get(key) {
                    return Some(KeyAction::ViewSpecific(action.clone()));
                }
            }
        }
        
        // 3. 上下文按键最后
        if let Some(context_id) = current_context {
            if let Some(context_keymap) = self.context_keymaps.get(&context_id) {
                if let Some(action) = context_keymap.get(key) {
                    return Some(KeyAction::Contextual(action.clone()));
                }
            }
        }
        
        None
    }
}
```

### 3.4 按键冲突处理策略

```rust
/// 按键冲突处理（推导自 architecture.md:422-432）
impl KeymapRegistry {
    pub fn register_key_with_conflict_handling(
        &mut self,
        key: KeyEvent,
        action: KeyAction,
        scope: KeymapScope
    ) -> Result<(), KeymapConflictError> {
        let keymap = match scope {
            KeymapScope::Global => &mut self.global_keymap,
            KeymapScope::View(view_id) => self.view_keymaps.entry(view_id).or_default(),
            KeymapScope::Context(context_id) => self.context_keymaps.entry(context_id).or_default(),
        };
        
        // 检查是否已存在冲突
        if keymap.contains_key(&key) {
            // 根据冲突策略决定行为
            match self.conflict_strategy {
                ConflictStrategy::Override => {
                    keymap.insert(key, action);
                    Ok(())
                }
                ConflictStrategy::Preserve => {
                    Err(KeymapConflictError::ConflictExists(key))
                }
                ConflictStrategy::Warn => {
                    eprintln!("Warning: Overriding key binding for {:?}", key);
                    keymap.insert(key, action);
                    Ok(())
                }
            }
        } else {
            keymap.insert(key, action);
            Ok(())
        }
    }
}

pub enum ConflictStrategy {
    Override,
    Preserve,
    Warn,
}
```

### 3.5 Codex vs Loom 按键映射对比

| 维度 | Codex CLI | Loom TUI |
|------|-----------|----------|
| **按键绑定** | `keymap.rs` 运行时动态 | `KeymapRegistry` 分层管理 |
| **作用域** | 全局 + 视图 | 全局 + 视图 + 上下文 |
| **冲突处理** | 后绑定覆盖前绑定 | 可配置冲突策略 |
| **扩展性** | 静态注册表 | 动态注册 + 作用域隔离 |

**Codex 按键特点**（参考 `reference-codex.md` 第 3 节，实际代码结构为逐 match 处理而非链式注册）：
```rust
// Codex 通过 bottom_pane 视图的 match 匹配处理按键
// 例如 ChatComposer::handle_key_event() 中：
// KeyCode::Enter => 提交
// KeyCode::Up | KeyCode::Down => 历史导航
// KeyCode::Char('/') => slash 命令
```

**Loom 按键优势**：
- 三层作用域（全局、视图、上下文）更精细控制
- 冲突处理策略灵活（覆盖/保留/警告）
- 支持上下文感知的按键映射
- 更好的扩展性和可维护性

---

## 4. 对抗性验证

### 4.1 边缘情况分析

#### 4.1.1 多修饰键同时按下

**风险场景**：用户同时按下 `Ctrl+Alt+Shift+Del` 或类似的组合键。

**防御措施**：
```rust
/// 修饰键组合验证（防御性编程）
fn validate_key_event(key: &KeyEvent) -> bool {
    // 拒绝过多的修饰键组合
    let modifier_count = [
        key.modifiers.control,
        key.modifiers.alt,
        key.modifiers.shift,
    ].iter().filter(|&&b| b).count();
    
    if modifier_count > 2 {
        tracing::warn!("Ignoring overly complex key combination: {:?}", key);
        return false;
    }
    
    // 特殊处理危险组合键
    match (key.code, key.modifiers.control, key.modifiers.alt) {
        (KeyCode::Char('c'), true, true) => {
            // Ctrl+Alt+C 可能被系统保留，谨慎处理
            tracing::warn!("Ignoring potentially system-reserved key: Ctrl+Alt+C");
            return false;
        }
        _ => true
    }
}
```

**Codex 参考**：Codex 对此类情况处理较少，主要依赖 crossterm 的默认行为。

#### 4.1.2 终端转义序列注入

**风险场景**：恶意输入包含 ANSI 转义序列，可能导致终端行为异常。

**防御措施**：
```rust
/// 转义序列过滤（paste 事件处理）
fn sanitize_paste_content(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars();
    
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // 检测到转义字符，跳过后续的转义序列
            if let Some(next) = chars.next() {
                if next.is_ascii_alphabetic() || next == '[' {
                    // 跳过转义序列的参数部分
                    while let Some(c) = chars.next() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            }
        } else if ch.is_control() && ch != '\n' && ch != '\r' && ch != '\t' {
            // 过滤掉其他控制字符（保留换行、回车、制表符）
            continue;
        } else {
            output.push(ch);
        }
    }
    
    output
}
```

**Codex 参考**：Codex 的 `clipboard_paste.rs` 没有明确的转义序列过滤，存在潜在风险。

#### 4.1.3 大文本粘贴

**风险场景**：用户粘贴大量文本（如整个文件内容），可能导致内存溢出或渲染卡顿。

**防御措施**：
```rust
/// 粘贴内容大小限制（防御性编程）
const MAX_PASTE_SIZE: usize = 100_000; // 100KB

fn handle_paste_event(&mut self, text: String) -> Handled {
    // 检查粘贴内容大小
    if text.len() > MAX_PASTE_SIZE {
        let _ = self.show_error_message(format!(
            "粘贴内容过大 ({} 字节)，已截断至 {} 字节",
            text.len(), MAX_PASTE_SIZE
        ));
        let truncated = text.chars().take(MAX_PASTE_SIZE).collect::<String>();
        self.chat_composer.insert_text(&truncated);
    } else {
        self.chat_composer.insert_text(&text);
    }
    
    Handled::Handled
}

/// 分块渲染大粘贴内容
impl ChatComposer {
    pub fn insert_text(&mut self, text: &str) {
        // 分块插入，避免渲染阻塞
        let chunk_size = 1000;
        for chunk in text.as_bytes().chunks(chunk_size) {
            if let Ok(chunk_str) = std::str::from_utf8(chunk) {
                self.content.push_str(chunk_str);
            }
        }
        
        // 触发延迟渲染
        self.request_delayed_render();
    }
}
```

**Codex 参考**：Codex 没有明确的粘贴大小限制，大文本可能导致性能问题。

### 4.2 失败模式分析

#### 4.2.1 事件丢失

**风险场景**：事件队列满时新事件被丢弃，或 `EventBroker` 崩溃导致事件丢失。

**防御措施**：
```rust
/// 事件队列容量管理和背压
pub struct EventBroker {
    event_queue: mpsc::Sender<TuiEvent>,
    dropped_events: Arc<AtomicU64>,
}

impl EventBroker {
    pub fn send_event(&self, event: TuiEvent) -> Result<(), EventError> {
        match self.event_queue.try_send(event) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(event)) => {
                // 队列满，丢弃事件并记录
                self.dropped_events.fetch_add(1, Ordering::Relaxed);
                tracing::error!("Event queue full, dropping event: {:?}", event);
                
                // 对于关键事件（如中断），尝试阻塞发送
                if is_critical_event(&event) {
                    self.event_queue.blocking_send(event)
                        .map_err(|_| EventError::QueueFull)?;
                }
                
                Err(EventError::QueueFull)
            }
            Err(TrySendError::Closed(_)) => {
                Err(EventError::QueueClosed)
            }
        }
    }
    
    fn is_critical_event(event: &TuiEvent) -> bool {
        matches!(event, TuiEvent::Key(KeyEvent { 
            code: KeyCode::Char('c' | 'z'), 
            modifiers: KeyModifiers { control: true, .. }, 
            .. 
        }))
    }
}
```

**Codex 参考**：Codex 的 JSON-RPC 机制中，如果 stdout 写入失败，事件会丢失。

#### 4.2.2 重复事件

**风险场景**：终端发送重复的 `Resize` 事件或键盘事件，导致重复处理。

**防御措施**：
```rust
/// 事件去重和节流
pub struct EventDeduplicator {
    last_resize: Option<(u16, u16)>,
    resize_timeout: Option<Instant>,
    key_event_buffer: HashMap<KeyEvent, Instant>,
}

impl EventDeduplicator {
    pub fn deduplicate_resize(&mut self, columns: u16, rows: u16) -> Option<TuiEvent> {
        // 忽略重复的 resize 事件（短时间内相同尺寸）
        if let Some((last_cols, last_rows)) = self.last_resize {
            if last_cols == columns && last_rows == rows {
                if let Some(timeout) = self.resize_timeout {
                    if timeout.elapsed() < Duration::from_millis(100) {
                        return None; // 忽略重复
                    }
                }
            }
        }
        
        self.last_resize = Some((columns, rows));
        self.resize_timeout = Some(Instant::now());
        Some(TuiEvent::Resize { columns, rows })
    }
    
    pub fn deduplicate_key_event(&mut self, key: KeyEvent) -> Option<TuiEvent> {
        let now = Instant::now();
        
        // 检查是否为短时间内重复的按键
        if let Some(&last_time) = self.key_event_buffer.get(&key) {
            if now.duration_since(last_time) < Duration::from_millis(50) {
                return None; // 忽略去抖
            }
        }
        
        self.key_event_buffer.insert(key, now);
        Some(TuiEvent::Key(key))
    }
}
```

**Codex 参考**：Codex 没有明确的事件去重机制，依赖终端的默认行为。

#### 4.2.3 时序竞争

**风险场景**：`^Z` 暂停和 `Ctrl+C` 中断同时发生，或状态更新和渲染产生竞争。

**防御措施**：
```rust
/// 优先级锁和原子操作
pub struct AppState {
    state: Arc<Mutex<AppStateMachine>>,
    cancel_flag: Arc<AtomicBool>,
    suspend_flag: Arc<AtomicBool>,
}

impl AppState {
    pub fn handle_interrupt(&self) {
        // 原子设置中断标志，避免竞争
        self.cancel_flag.store(true, Ordering::SeqCst);
        
        // 使用超时锁，避免死锁
        if let Ok(mut state) = tokio::time::timeout(
            Duration::from_millis(100),
            self.state.lock()
        ).await {
            state.transition_to(AppState::Interrupted);
        } else {
            tracing::error!("Failed to acquire state lock for interrupt");
        }
    }
    
    pub fn handle_suspend(&self) {
        // 中断优先于暂停
        if self.cancel_flag.load(Ordering::SeqCst) {
            tracing::warn!("Interrupt pending, ignoring suspend request");
            return;
        }
        
        self.suspend_flag.store(true, Ordering::SeqCst);
    }
}
```

**Codex 参考**：Codex 的 `turn/cancel` 机制相对简单，竞争条件处理不够完善。

### 4.3 攻击面分析

#### 4.3.1 模拟按键注入

**风险场景**：恶意代码模拟键盘事件，注入命令或触发危险操作。

**防御措施**：
```rust
/// 事件来源验证（可选，根据安全需求）
pub struct TuiEventStream {
    trusted_source: bool, // 是否来自可信终端
}

impl TuiEventStream {
    pub fn validate_event_source(&self, event: &TuiEvent) -> bool {
        // 对于高安全模式，验证事件来源
        if !self.trusted_source {
            if matches!(event, TuiEvent::Key(KeyEvent { 
                code: KeyCode::Char('c' | 'z' | 'd'), 
                modifiers: KeyModifiers { control: true, .. }, 
                .. 
            })) {
                tracing::warn!("Blocking privileged key from untrusted source");
                return false;
            }
        }
        true
    }
}
```

**Codex 参考**：Codex 的 stdio 机制无法验证事件来源，存在注入风险。

#### 4.3.2 终端转义序列注入攻击

**风险场景**：通过精心构造的转义序列执行任意终端命令。

**防御措施**：
```rust
/// 严格的转义序列过滤
pub struct TerminalSecurityLayer {
    allowed_ansi_codes: HashSet<char>,
}

impl TerminalSecurityLayer {
    pub fn sanitize_output(&self, input: &str) -> String {
        let mut output = String::new();
        let mut chars = input.chars().peekable();
        
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                // 处理转义序列
                match chars.next() {
                    Some('[') => {
                        // CSI 序列
                        let mut params = String::new();
                        while let Some(&c) = chars.peek() {
                            if c.is_ascii_digit() || c == ';' {
                                params.push(chars.next().unwrap());
                            } else if c.is_ascii_alphabetic() {
                                let final_char = chars.next().unwrap();
                                // 只允许安全的 ANSI 码
                                if self.allowed_ansi_codes.contains(&final_char) {
                                    output.push_str(&format!("\x1b[{}{}", params, final_char));
                                }
                                break;
                            } else {
                                break;
                            }
                        }
                    }
                    _ => {
                        // 其他转义序列，过滤
                    }
                }
            } else {
                output.push(ch);
            }
        }
        
        output
    }
}
```

**Codex 参考**：Codex 没有输出过滤，转义序列直接传递给终端。

### 4.4 设计限制与未覆盖场景

#### 4.4.1 当前设计的限制

| 限制 | 影响 | 缓解措施 |
|------|------|----------|
| **单线程事件循环** | 大量事件处理可能阻塞 | 使用异步操作，将耗时任务移到后台 |
| **无鼠标支持** | 无法使用鼠标操作 | 计划在后续版本添加鼠标事件支持 |
| **有限的国际输入** | 复杂输入法支持有限 | 依赖终端的输入法支持 |
| **无触摸支持** | 触摸设备无法使用 | TUI 设计预期为键盘交互 |
| **固定按键映射** | 无法动态重新绑定 | 计划添加用户自定义按键映射 |

#### 4.4.2 未覆盖的输入场景

1. **Unicode 组合字符**：某些 Unicode 组合字符可能无法正确处理
2. **复杂输入法**：日文/中文输入法的实时预览支持有限
3. **多媒体键**：音量、亮度等系统按键无法捕获
4. **游戏手柄/触摸板**：非标准输入设备支持有限
5. **语音输入**：语音转文本的事件流未设计

#### 4.4.3 性能边界

```rust
/// 性能边界测试和监控
pub struct PerformanceMonitor {
    event_processing_times: VecDeque<Duration>,
    render_times: VecDeque<Duration>,
}

impl PerformanceMonitor {
    pub fn check_performance_health(&self) -> PerformanceHealth {
        let avg_event_time = self.average_event_time();
        let avg_render_time = self.average_render_time();
        
        match (avg_event_time, avg_render_time) {
            (event, render) if event > Duration::from_millis(16) || render > Duration::from_millis(16) => {
                PerformanceHealth::Degraded {
                    event_processing_ms: event.as_millis(),
                    render_ms: render.as_millis(),
                }
            }
            (event, render) if event > Duration::from_millis(8) || render > Duration::from_millis(8) => {
                PerformanceHealth::Warning
            }
            _ => PerformanceHealth::Healthy
        }
    }
}
```

---

## 5. 总结

Loom TUI 的事件系统与输入处理机制基于 Codex CLI 的经验教训，在事件类型定义、路由架构、按键映射和安全性方面进行了显著改进：

### 5.1 核心改进

1. **原生终端事件流**：从 Codex 的 JSON-RPC 机制升级为直接终端事件捕获，减少延迟和序列化开销
2. **分层按键映射**：全局、视图、上下文三层作用域，提供更精细的控制
3. **事件优先级系统**：确保关键操作（如中断、暂停）始终优先响应
4. **安全性增强**：转义序列过滤、事件去重、大小限制等防御措施

### 5.2 与 Codex 的差异

| 方面 | Codex | Loom |
|------|-------|------|
| **事件传输** | JSON-RPC over stdio | 内存中 EventBroker |
| **中断机制** | `turn/cancel` 方法 | 原生 Ctrl+C 事件 |
| **按键映射** | 静态注册表 | 动态分层注册 |
| **安全性** | 基础 | 多层防御 |
| **扩展性** | 中等 | 高 |

### 5.3 未来演进方向

1. **鼠标支持**：添加鼠标事件处理和交互组件
2. **动态按键映射**：支持用户自定义按键绑定
3. **插件化事件处理器**：允许第三方扩展事件处理逻辑
4. **性能优化**：更智能的事件节流和批处理
5. **国际化支持**：更好的多语言输入支持

通过这种基于 Codex 源码推导和对抗性验证的设计方法，Loom TUI 的事件系统在保持交互体验的同时，提供了更强的安全性、可靠性和扩展性。