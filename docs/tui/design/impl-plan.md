# TUI 实现方案：基于 Codex CLI 架构分析

## 概述

本文档基于 Codex CLI TUI 的架构分析，提出 Loom CLI 的 TUI 实现方案。聚焦于**可执行的具体代码方案**，包括架构设计、模块划分、核心接口定义、以及分阶段实现计划。

**参考文档**：
- `docs/codex-tui/product/product.md` — 产品定位
- `docs/codex-tui/design/interaction.md` — 交互设计
- `docs/codex-tui/design/engineering.md` — 工程架构

---

## 1. 现状分析：Loom CLI 当前显示系统

### 1.1 当前架构

Loom CLI 当前使用**纯文本 ANSI 输出**，通过 `apps/cli/src/display/` 模块实现：

```
display/
├── event_handler.rs        # 流事件回调 → 格式化输出
├── format.rs               # 格式化工具
├── markdown.rs             # markdown 渲染
├── mod.rs                  # 公共导出
├── panel_format.rs         # 面板格式（_CATEGORY message）
├── spinner.rs              # 单行 \r 动画 spinner
├── streaming_markdown.rs   # 流式 markdown 渲染
├── terminal.rs             # 终端检测 + ANSI 包装
├── tool_preview.rs         # 工具预览格式
└── tool_summary.rs         # 工具调用摘要格式
```

### 1.2 当前能力

| 特性 | 状态 | 说明 |
|------|------|------|
| 流式输出 | ✅ 已有 | `StreamingMarkdownRenderer` |
| Spinner | ✅ 已有 | 单行 \r 动画，10 帧 dots |
| 面板格式 | ✅ 已有 | `_CATEGORY  message` 格式 |
| 终端检测 | ✅ 已有 | `get_terminal_width()`, `is_tty()` |
| 分页器 | ✅ 已有 | `minus` crate |
| **内联视图** | ❌ 无 | 输出是纯文本流，无固定区域 |
| **交互式输入** | ❌ 无 | 无输入框，仅通过 `run` 命令 |
| **审批弹窗** | ❌ 无 | 无交互式审批 |
| **事件驱动** | ⚠️ 部分 | 回调模式，非 `tokio::select!` |
| **^Z 暂停** | ❌ 无 | 无 job control |
| **桌面通知** | ❌ 无 | 无通知系统 |

### 1.3 关键差异：Codex vs Loom

| 维度 | Codex CLI | Loom CLI |
|------|-----------|----------|
| 交互模式 | 始终运行，对话式 | 单次运行，输出后退出 |
| TUI 框架 | ratatui + crossterm | 纯 ANSI |
| 输入方式 | 交互式输入框 (`ChatComposer`) | CLI 参数 + `--prompt` |
| 运行模式 | 守护进程式 | 一次性命令 |
| 后端通信 | in-process app server | 直接调用 agent |

---

## 2. 目标架构

### 2.1 设计原则

1. **渐进增强**：保留现有 ANSI 输出路径，TUI 作为可选模式（`--tui` 或自动检测）
2. **模块化**：每个功能独立文件，避免 Codex 的 `chatwidget.rs` 单文件过大问题
3. **可测试**：核心逻辑与终端渲染分离，可单元测试
4. **最小依赖**：仅添加必要的依赖（ratatui, crossterm），不引入大型框架

### 2.2 模块结构

```
apps/cli/src/
├── main.rs                    # 入口：CLI 参数解析，选择 TUI 或传统模式
├── lib.rs                     # 模块声明
├── tui/                       # TUI 模块（新增，可选编译）
│   ├── mod.rs                 # 公共导出
│   ├── app.rs                 # App 主循环（tokio::select! 事件分发）
│   ├── terminal.rs            # 终端初始化 + 内联视图管理
│   │   ├── init()             # 初始化终端（raw mode、bracketed paste）
│   │   ├── draw()             # 内联视图渲染
│   │   └── job_control()      # ^Z 暂停/恢复
│   ├── event.rs               # 事件系统（TuiEvent, EventBroker）
│   ├── render.rs              # Renderable trait + 布局组件
│   │   ├── trait Renderable
│   │   ├── ColumnRenderable
│   │   └── FlexRenderable
│   ├── pane.rs                # 栈式面板管理
│   │   ├── trait PaneView
│   │   └── PaneStack
│   ├── composer.rs            # 输入框（ChatComposer 简化版）
│   ├── history.rs             # 历史行插入
│   ├── spinner.rs             # Spinner 集成（复用现有帧动画）
│   ├── status.rs              # 状态指示器
│   ├── notification.rs        # 桌面通知
│   └── diff.rs                # 差异渲染
├── run/                       # 现有：agent 运行逻辑（不变）
└── display/                   # 现有：ANSI 输出（保留，非 TUI 模式使用）
```

### 2.3 运行模式选择

```
main.rs
  │
  ├── CLI 参数解析
  │
  ├── 交互模式（--interactive / -i）
  │   → 启动 TUI 循环
  │   → 用户输入 → 提交 → 显示结果 → 继续
  │
  ├── 单次运行（默认）
  │   → 使用现有 display/ 模块
  │   → 纯文本 ANSI 输出
  │   → 退出
  │
  └── 审批模式（--approve）
      → 混合模式：ANSI 输出 + 弹窗式审批
      → 使用 bottom_pane 的审批视图
```

---

## 3. 核心接口定义

### 3.1 Renderable Trait

所有 UI 组件实现此 trait，与 Codex 的 `Renderable` 一致：

```rust
// tui/render.rs
pub trait Renderable {
    /// 渲染到 ratatui buffer
    fn render(&self, area: Rect, buf: &mut Buffer);
    /// 告知布局系统需要多少高度
    fn desired_height(&self, width: u16) -> u16;
    /// 光标位置（用于输入框）
    fn cursor_pos(&self, _area: Rect) -> Option<(u16, u16)>;
    /// 光标样式
    fn cursor_style(&self, _area: Rect) -> SetCursorStyle;
}
```

### 3.2 PaneView Trait

栈式面板视图，与 Codex 的 `BottomPaneView` 对应：

```rust
// tui/pane.rs
pub trait PaneView: Renderable {
    fn handle_key_event(&mut self, key: KeyEvent) -> Handled;
    fn is_complete(&self) -> bool { false }
    fn on_ctrl_c(&mut self) -> CtrlCAction { CtrlCAction::NotHandled }
    fn view_id(&self) -> Option<&'static str> { None }
}

pub enum Handled {
    Handled,
    NotHandled,
}

pub enum CtrlCAction {
    NotHandled,
    Handled,
    Cancel,
}
```

### 3.3 TuiEvent

```rust
// tui/event.rs
pub enum TuiEvent {
    Key(KeyEvent),
    Paste(String),
    Resize(Size),
    Draw,
    Resume,  // 从 ^Z 恢复
}
```

### 3.4 App 主循环

```rust
// tui/app.rs
pub struct App {
    tui: Terminal,
    chat_history: Vec<HistoryCell>,
    active_cell: Option<ActiveCell>,
    pane_stack: PaneStack,
    // ...
}

impl App {
    pub async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Some(event) = self.tui.event_stream().next() => {
                    match event {
                        TuiEvent::Key(key) => self.handle_key(key),
                        TuiEvent::Resize(size) => self.handle_resize(size),
                        TuiEvent::Draw => self.render(),
                        TuiEvent::Resume => self.handle_resume(),
                        _ => {}
                    }
                }
                Some(result) = self.agent_rx.recv() => {
                    self.handle_agent_result(result);
                }
            }
        }
    }
}
```

---

## 4. 实现阶段

### Phase 1: 基础设施（2-3 天）

**目标**：建立 TUI 基础运行环境，不依赖 ratatui

| 任务 | 文件 | 说明 |
|------|------|------|
| 1.1 终端初始化 | `tui/terminal.rs` | raw mode、bracketed paste、panic hook |
| 1.2 事件系统 | `tui/event.rs` | `TuiEvent` + `EventBroker` + `TuiEventStream` |
| 1.3 内联视图 | `tui/terminal.rs` | `draw()` + flush_pending_history_lines |
| 1.4 历史行插入 | `tui/history.rs` | `insert_history_lines()` + `PendingHistoryLines` |
| 1.5 状态管理 | `tui/app.rs` | `App` 结构体 + `run()` 主循环 |

**依赖**：`crossterm`（raw mode, event stream）

**交付物**：
- 终端初始化成功，raw mode 可用
- 事件流可接收按键
- 内联视图可在终端中显示固定区域

### Phase 2: 渲染系统（2-3 天）

**目标**：引入 ratatui，建立渲染管线

| 任务 | 文件 | 说明 |
|------|------|------|
| 2.1 添加 ratatui 依赖 | `Cargo.toml` | `ratatui` + `crossterm` backend |
| 2.2 Renderable trait | `tui/render.rs` | 核心 trait + 布局组件 |
| 2.3 渲染管线 | `tui/terminal.rs` | `draw_with_size()` + SynchronizedUpdate |
| 2.4 Resize 处理 | `tui/terminal.rs` | `update_inline_viewport()` |
| 2.5 状态指示器 | `tui/status.rs` | 状态行 + spinner 集成 |

**依赖**：`ratatui`

**交付物**：
- 聊天区域可渲染文本
- Resize 时 viewport 正确调整
- Status 行显示 AI 状态

### Phase 3: 交互系统（3-4 天）

**目标**：实现输入框 + 审批弹窗 + 面板栈

| 任务 | 文件 | 说明 |
|------|------|------|
| 3.1 PaneView trait | `tui/pane.rs` | trait + PaneStack |
| 3.2 ChatComposer | `tui/composer.rs` | 输入框（TextArea + 历史 + slash 命令） |
| 3.3 审批弹窗 | `tui/pane/approval.rs` | 审批视图（Y/N/D/A） |
| 3.4 选择列表 | `tui/pane/selection.rs` | 通用选择列表 |
| 3.5 配置视图 | `tui/pane/settings.rs` | 模型选择、配置修改 |

**交付物**：
- 用户可输入文本并提交
- AI 可请求审批，用户可交互式响应
- 底部面板栈正常工作

### Phase 4: 集成与优化（2-3 天）

**目标**：与现有 agent 运行系统集成，添加高级功能

| 任务 | 文件 | 说明 |
|------|------|------|
| 4.1 Agent 集成 | `tui/app.rs` | 对接现有 `run_agent()` |
| 4.2 流式输出 | `tui/streaming.rs` | 流式 markdown 渲染到 TUI |
| 4.3 多代理 | `tui/agent_feed.rs` | 子代理状态展示 |
| 4.4 Job control | `tui/job_control.rs` | `^Z` 暂停/恢复 |
| 4.5 桌面通知 | `tui/notification.rs` | 失焦时通知 |
| 4.6 回放 | `tui/replay.rs` | 对话历史回放 |

**交付物**：
- 完整 TUI 体验：输入 → 提交 → AI 处理 → 结果显示
- `^Z` 暂停/恢复
- 桌面通知

---

## 5. 关键决策

### 5.1 内联视图 vs 全屏

**决策：内联视图**（与 Codex 一致）

理由：
- Loom 是 CLI 工具，用户需要在终端中同时使用其他命令
- 内联视图保留终端历史，适合非独占场景
- 退出后终端状态完全恢复

### 5.2 ratatui vs 纯 ANSI

**决策：ratatui**

理由：
- 提供成熟的 buffer、布局、widget 系统
- 避免重复实现布局算法
- 社区活跃，维护成本低
- Codex 已验证其可靠性（237K 行代码）

### 5.3 运行模式

**决策：双模式**（TUI + 传统 ANSI）

理由：
- 向后兼容：现有脚本和管道继续工作
- 渐进采用：用户可逐步过渡
- 降级支持：非 TTY 环境自动降级

### 5.4 模块拆分策略

**决策：避免单文件过大**

教训来自 Codex 的 `chatwidget.rs`（83KB 单文件）。Loom 的 TUI 模块按职责拆分为独立文件：
- 每个文件不超过 500 行
- 复杂模块拆分为子目录（如 `pane/`）
- 与现有 `display/` 模块共享公共工具函数

---

## 6. 与现有系统的集成

### 6.1 复用现有组件

| 现有组件 | 复用方式 | 说明 |
|----------|----------|------|
| `display/spinner.rs` | 直接复用帧动画 | `SpinnerTrait` 保持兼容 |
| `display/terminal.rs` | 复用终端检测 | `get_terminal_width()`, `is_tty()` |
| `display/panel_format.rs` | TUI 中渲染为面板 | 内容格式化逻辑复用 |
| `display/streaming_markdown.rs` | TUI 中渲染为流式文本 | 渲染器复用 |
| `display/markdown.rs` | 渲染到 history cell | markdown → ratatui text |
| `run/agent.rs` | 回调适配 | 将事件回调转为 TUI 更新 |

### 6.2 条件编译

TUI 作为可选特性，通过 Cargo feature flags 控制：

```toml
# Cargo.toml
[features]
default = []
tui = ["ratatui", "crossterm"]
```

```rust
// main.rs
#[cfg(feature = "tui")]
mod tui;
```

### 6.3 事件适配

当前 `run/agent.rs` 使用回调模式：

```rust
type StreamCallback = Arc<Mutex<dyn FnMut(Value) + Send>>;
```

适配方案：TUI 模式下，回调写入 `tokio::sync::mpsc` channel：

```rust
// tui 模式下
let (tx, mut rx) = tokio::sync::mpsc::channel(256);
let callback = move |event: Value| {
    tx.blocking_send(event).ok();
};

// App 主循环
tokio::select! {
    Some(event) = rx.recv() => {
        self.handle_agent_event(event);
    }
    // ...
}
```

---

## 7. 第一阶段详细实现

### 7.1 terminal.rs — 终端初始化

```rust
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use crossterm::event::{EnableBracketedPaste, DisableBracketedPaste};

pub fn init() -> Result<InitializedTerminal> {
    enable_raw_mode()?;
    // 设置 bracketed paste
    // 设置 panic hook（恢复终端）
    // 探测光标位置（viewport 起始点）
    // 返回 InitializedTerminal
}

pub fn restore() -> Result<()> {
    disable_raw_mode()?;
    // 恢复终端到初始状态
}

pub struct Tui {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    pending_history: Vec<HistoryLine>,
    viewport: Viewport,
    // ...
}

impl Tui {
    pub fn draw(&mut self, height: u16, f: impl FnOnce(&mut Frame)) -> Result<()> {
        // 1. 处理 ^Z 恢复
        // 2. 更新 viewport
        // 3. flush_pending_history_lines()
        // 4. terminal.draw_with_size(viewport, f)
    }
}
```

### 7.2 event.rs — 事件系统

```rust
pub enum TuiEvent {
    Key(KeyEvent),
    Paste(String),
    Resize(Size),
    Draw,
    Resume,
}

pub struct EventBroker {
    // Arc 共享，支持 pause/resume
}

pub fn event_stream() -> impl Stream<Item = TuiEvent> {
    // 包装 crossterm EventStream
    // 处理焦点事件、粘贴解析、resize 检测
}
```

### 7.3 app.rs — 主循环骨架

```rust
pub struct App {
    tui: Tui,
    composer: Composer,
    history: Vec<HistoryCell>,
    status: StatusBar,
    agent_tx: mpsc::Sender<Value>,
    agent_rx: mpsc::Receiver<Value>,
}

impl App {
    pub async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                Some(event) = self.tui.event_stream().next() => {
                    match event {
                        TuiEvent::Key(key) => self.handle_key(key),
                        TuiEvent::Resize(size) => self.handle_resize(size),
                        TuiEvent::Draw => self.render(),
                        TuiEvent::Resume => self.handle_resume(),
                        TuiEvent::Paste(text) => self.composer.insert_text(&text),
                    }
                }
                Some(event) = self.agent_rx.recv() => {
                    self.handle_agent_event(event);
                }
            }
            self.render();
        }
    }
}
```

---

## 8. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| ratatui 版本更新 API 变化 | 维护成本 | 锁定版本，依赖最小功能集 |
| 内联视图在特殊终端（Zellij）异常 | 兼容性问题 | 延后处理，优先支持标准终端 |
| 与现有 display/ 模块冲突 | 代码重复 | 明确职责边界，共享工具函数 |
| 性能（大对话历史渲染） | 卡顿 | 虚拟化渲染，只渲染可见区域 |
| 多平台兼容（Windows） | 功能缺失 | 条件编译 + 降级方案 |

---

## 9. 附录：Cargo.toml 新增依赖

```toml
# 可选依赖（feature = "tui" 时启用）
[dependencies]
ratatui = { version = "0.28", optional = true, features = ["crossterm"] }
crossterm = { version = "0.28", optional = true, features = ["event-stream", "bracketed-paste"] }

[features]
default = []
tui = ["ratatui", "crossterm"]
```

---

## 10. 总结

本方案基于 Codex CLI TUI 的成熟架构（237K 行已验证代码），为 Loom CLI 设计了一套渐进式 TUI 实现路径。核心策略：

1. **双模式运行**：TUI 模式 + 传统 ANSI 模式并行
2. **内联视图**：不占全屏，融入终端工作流
3. **模块化拆分**：避免 Codex 的单文件过大问题
4. **渐进实现**：4 个阶段，从基础设施到完整交互

第一阶段（Phase 1-2）即可交付可用的 TUI 基础体验，后续阶段逐步增强交互能力。
