# Phase 2: 渲染系统实现方案

## 概述

Phase 2 引入 ratatui，建立 TUI 渲染管线。本阶段聚焦于**视觉呈现**——实现 Renderable trait、布局组件、对话历史渲染、差异渲染，以及状态指示器。

**目标**：在终端中渲染格式化的聊天区域，支持 markdown 渲染、差异高亮、Spinner 动画。

**预计文件**：8-10 个文件，~1200 行代码

---

## 1. 文件清单

| 文件 | 职责 | 预计行数 | 依赖 |
|------|------|----------|------|
| `tui/render.rs` | Renderable trait + 布局组件 | ~150 | ratatui |
| `tui/terminal.rs` | 渲染管线升级（draw_with_size + SynchronizedUpdate） | ~200 | ratatui |
| `tui/viewport.rs` | Resize 处理升级 | ~50 | 无 |
| `tui/status.rs` | 状态指示器 + Spinner 集成 | ~150 | ratatui |
| `tui/history_cell.rs` | 对话历史 cell 渲染 | ~200 | ratatui |
| `tui/streaming.rs` | 流式输出渲染 | ~200 | ratatui |
| `tui/diff.rs` | 文件差异渲染 | ~150 | ratatui |
| `tui/spinner.rs` | Spinner 动画适配（复用现有帧动画） | ~100 | 无 |

---

## 2. 核心实现

### 2.1 Renderable Trait 与布局组件 (`tui/render.rs`)

#### 2.1.1 Renderable Trait

```rust
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::SetCursorStyle,
};

/// 所有 UI 组件必须实现的渲染接口
pub trait Renderable {
    /// 渲染到 ratatui buffer
    fn render(&self, area: Rect, buf: &mut Buffer);

    /// 告知布局系统需要多少高度
    fn desired_height(&self, width: u16) -> u16;

    /// 光标位置（用于输入框）
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        None
    }

    /// 光标样式
    fn cursor_style(&self, area: Rect) -> SetCursorStyle {
        SetCursorStyle::DefaultUserShape
    }
}
```

#### 2.1.2 布局组件

```rust
/// 垂直堆叠布局：多个 Renderable 从上到下排列
pub struct ColumnRenderable<'a> {
    children: Vec<&'a dyn Renderable>,
}

impl<'a> Renderable for ColumnRenderable<'a> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut y = area.y;
        for child in &self.children {
            let height = child.desired_height(area.width).min(area.height.saturating_sub(y - area.y));
            let child_area = Rect::new(area.x, y, area.width, height);
            child.render(child_area, buf);
            y += height;
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.children.iter().map(|c| c.desired_height(width)).sum()
    }
}

/// 弹性布局：按比例分配垂直空间
pub struct FlexRenderable<'a> {
    children: Vec<(&'a dyn Renderable, u16)>, // (renderable, flex_weight)
}

impl<'a> Renderable for FlexRenderable<'a> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let total_weight: u16 = self.children.iter().map(|(_, w)| w).sum();
        if total_weight == 0 {
            return;
        }
        let mut y = area.y;
        for (child, weight) in &self.children {
            let height = area.height * weight / total_weight;
            let child_area = Rect::new(area.x, y, area.width, height);
            child.render(child_area, buf);
            y += height;
        }
    }

    fn desired_height(&self, _width: u16) -> u16 {
        // Flex 布局占用所有可用空间
        0
    }
}

/// 内边距包装
pub struct InsetRenderable<'a> {
    inner: &'a dyn Renderable,
    top: u16,
    bottom: u16,
    left: u16,
    right: u16,
}

impl<'a> Renderable for InsetRenderable<'a> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let inner_area = Rect::new(
            area.x + self.left,
            area.y + self.top,
            area.width.saturating_sub(self.left + self.right),
            area.height.saturating_sub(self.top + self.bottom),
        );
        self.inner.render(inner_area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.inner.desired_height(width.saturating_sub(self.left + self.right))
            + self.top + self.bottom
    }
}
```

### 2.2 渲染管线升级 (`tui/terminal.rs`)

#### 2.2.1 Terminal 结构体升级

```rust
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
    Frame,
};

pub struct TuiTerminal {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    viewport: Viewport,
    pending_history: PendingHistory,
}

impl TuiTerminal {
    /// 创建新的 TUI 终端（在 init() 之后调用）
    pub fn new(viewport: Viewport) -> Result<Self> {
        let backend = CrosstermBackend::new(stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal,
            viewport,
            pending_history: PendingHistory::new(),
        })
    }

    /// 主渲染方法
    pub fn draw(
        &mut self,
        height: u16,
        draw_fn: impl FnOnce(&mut Frame),
    ) -> Result<()> {
        // 1. flush 待处理历史行
        self.pending_history.flush()?;

        // 2. 获取屏幕尺寸
        let screen_size = self.terminal.size()?;
        let area = Rect::new(0, 0, screen_size.width, height);

        // 3. 使用 SynchronizedUpdate 包裹渲染
        stdout().sync_update(|_| {
            self.terminal.draw_with_size(|frame| {
                frame.set_area(area);
                draw_fn(frame);
            }, screen_size)?;
            Ok(())
        })?;

        Ok(())
    }

    /// 处理 resize
    pub fn handle_resize(&mut self, new_size: Size) -> bool {
        self.viewport.handle_resize(new_size)
    }

    /// 插入历史行到终端滚动区域
    pub fn insert_history_line(&mut self, line: String, wrap: HistoryLineWrapPolicy) {
        self.pending_history.push(line, wrap);
    }
}
```

### 2.3 状态指示器 (`tui/status.rs`)

#### 2.3.1 状态枚举

```rust
/// AI 状态
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AiStatus {
    /// 空闲，等待用户输入
    Idle,
    /// 正在提交
    Submitting,
    /// AI 正在思考
    Thinking,
    /// AI 正在执行工具
    Executing,
    /// 等待用户审批
    WaitingApproval,
    /// 用户中断
    Interrupted,
    /// 发生错误
    Error,
}
```

#### 2.3.2 StatusBar 渲染

```rust
use ratatui::{
    widgets::{Paragraph, Block, Borders},
    style::{Style, Color},
};

/// 状态指示器 widget
pub struct StatusBar {
    status: AiStatus,
    spinner: Spinner, // 复用现有 spinner
}

impl Renderable for StatusBar {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let (text, style) = match self.status {
            AiStatus::Idle => (" 等待输入 ".into(), Style::default().fg(Color::Green)),
            AiStatus::Thinking => {
                format!(" 思考中 {} ", self.spinner.current_frame())
            }
            AiStatus::Executing => {
                format!(" 执行中 {} ", self.spinner.current_frame())
            }
            AiStatus::WaitingApproval => {
                " 等待审批 ".into()
            }
            AiStatus::Error => {
                " 错误 ".into()
            }
            AiStatus::Interrupted => {
                " 已中断 ".into()
            }
            _ => "".into(),
        };

        let paragraph = Paragraph::new(text)
            .style(style)
            .block(Block::default().borders(Borders::TOP));
        paragraph.render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}
```

### 2.4 对话历史 Cell 渲染 (`tui/history_cell.rs`)

#### 2.4.1 HistoryCell 类型

```rust
/// 已完成的对话单元
pub enum HistoryCell {
    /// 用户消息
    UserMessage {
        content: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// AI 回复
    AssistantMessage {
        content: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// 工具调用
    ToolCall {
        tool_name: String,
        args: serde_json::Value,
        result: Option<String>,
        status: ToolStatus,
    },
    /// 系统消息
    SystemMessage {
        content: String,
        style: SystemMessageStyle,
    },
}
```

#### 2.4.2 渲染实现

```rust
impl Renderable for HistoryCell {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        match self {
            HistoryCell::UserMessage { content, .. } => {
                let block = Block::default()
                    .title(" User ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan));
                let paragraph = Paragraph::new(content.as_str())
                    .block(block)
                    .wrap(Wrap { trim: false });
                paragraph.render(area, buf);
            }
            HistoryCell::AssistantMessage { content, .. } => {
                let block = Block::default()
                    .title(" Assistant ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green));
                let paragraph = Paragraph::new(content.as_str())
                    .block(block)
                    .wrap(Wrap { trim: false });
                paragraph.render(area, buf);
            }
            HistoryCell::ToolCall { tool_name, status, .. } => {
                let status_text = match status {
                    ToolStatus::Running => "◌ 运行中",
                    ToolStatus::Completed => "✓ 已完成",
                    ToolStatus::Failed => "✗ 失败",
                    ToolStatus::Pending => "○ 等待中",
                };
                let text = format!(" 工具: {} - {}", tool_name, status_text);
                let paragraph = Paragraph::new(text)
                    .style(Style::default().fg(Color::Yellow));
                paragraph.render(area, buf);
            }
            HistoryCell::SystemMessage { content, style } => {
                let color = match style {
                    SystemMessageStyle::Info => Color::Blue,
                    SystemMessageStyle::Warning => Color::Yellow,
                    SystemMessageStyle::Error => Color::Red,
                };
                let paragraph = Paragraph::new(content.as_str())
                    .style(Style::default().fg(color).italic());
                paragraph.render(area, buf);
            }
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        match self {
            HistoryCell::UserMessage { content, .. } => {
                // 计算文本行数 + 边框
                let line_count = content.lines().count() as u16;
                line_count + 2 // 上下边框
            }
            HistoryCell::AssistantMessage { content, .. } => {
                let line_count = content.lines().count() as u16;
                line_count + 2
            }
            HistoryCell::ToolCall { .. } => 1,
            HistoryCell::SystemMessage { content, .. } => {
                content.lines().count() as u16
            }
        }
    }
}
```

### 2.5 流式输出渲染 (`tui/streaming.rs`)

```rust
/// 正在流式输出的 AI 回复
pub struct StreamingCell {
    /// 已累积的内容
    content: String,
    /// 当前正在渲染的文本
    pending_text: String,
    /// 是否正在思考
    is_thinking: bool,
    /// 是否正在输出代码块
    in_code_block: bool,
}

impl StreamingCell {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            pending_text: String::new(),
            is_thinking: false,
            in_code_block: false,
        }
    }

    /// 追加文本 delta
    pub fn append_text(&mut self, delta: &str) {
        self.content.push_str(delta);
        self.pending_text.push_str(delta);
    }

    /// 刷新待处理文本到渲染
    pub fn flush(&mut self) {
        self.pending_text.clear();
    }
}

impl Renderable for StreamingCell {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(" Assistant (流式) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green));

        let display_text = if self.is_thinking {
            format!("◌ 思考中...\n{}", self.content)
        } else {
            self.content.clone()
        };

        let paragraph = Paragraph::new(display_text)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let line_count = self.content.lines().count() as u16;
        line_count + 2 // 上下边框
    }
}
```

### 2.6 差异渲染 (`tui/diff.rs`)

```rust
/// 文件差异渲染
pub fn render_diff(text: &str, area: Rect, buf: &mut Buffer) {
    for (i, line) in text.lines().enumerate() {
        let y = area.y + i as u16;
        if y >= area.y + area.height {
            break;
        }

        let (style, prefix) = if line.starts_with('+') {
            (Style::default().fg(Color::Green), "+")
        } else if line.starts_with('-') {
            (Style::default().fg(Color::Red), "-")
        } else if line.starts_with("@@") {
            (Style::default().fg(Color::Cyan).bold(), "@@")
        } else {
            (Style::default(), " ")
        };

        let text = Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(&line[1..], style),
        ]);
        text.render(Rect::new(area.x, y, area.width, 1), buf);
    }
}

/// 差异预览组件
pub struct DiffPreview {
    /// 差异文本（统一 diff 格式）
    diff_text: String,
    /// 文件名
    filename: String,
}

impl Renderable for DiffPreview {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(format!(" 差异: {} ", self.filename))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue));
        block.render(area, buf);

        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        render_diff(&self.diff_text, inner, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let line_count = self.diff_text.lines().count() as u16;
        line_count + 2 // 上下边框
    }
}
```

### 2.7 Spinner 适配 (`tui/spinner.rs`)

```rust
/// Spinner 动画适配器
/// 复用 apps/cli/src/display/spinner.rs 的帧动画
pub struct Spinner {
    frames: Vec<String>,
    current_frame: usize,
    tick_count: u64,
}

impl Spinner {
    pub fn new() -> Self {
        // 复用现有 spinner 帧定义
        let frames = vec![
            "⠋".to_string(), "⠙".to_string(), "⠹".to_string(),
            "⠸".to_string(), "⠼".to_string(), "⠴".to_string(),
            "⠦".to_string(), "⠧".to_string(), "⠇".to_string(), "⠏".to_string(),
        ];
        Self {
            frames,
            current_frame: 0,
            tick_count: 0,
        }
    }

    /// 每帧调用，推进动画
    pub fn tick(&mut self) {
        self.tick_count += 1;
        // 每 3 帧切换一次（约 300ms）
        if self.tick_count % 3 == 0 {
            self.current_frame = (self.current_frame + 1) % self.frames.len();
        }
    }

    /// 获取当前帧
    pub fn current_frame(&self) -> &str {
        &self.frames[self.current_frame]
    }
}
```

---

## 3. 集成点

### 3.1 App 渲染流程

```rust
// tui/app.rs
fn render(&mut self) {
    // 1. 计算所有组件的高度
    let mut renderables: Vec<&dyn Renderable> = Vec::new();
    
    // 历史 cells
    for cell in &self.history {
        renderables.push(cell as &dyn Renderable);
    }
    
    // 活跃 cell（流式输出）
    if let Some(active) = &self.active_cell {
        renderables.push(active as &dyn Renderable);
    }
    
    // 状态栏
    renderables.push(&self.status_bar as &dyn Renderable);
    
    // 底部面板（输入框/审批弹窗）
    renderables.push(&self.pane_stack as &dyn Renderable);
    
    // 2. 计算总高度
    let total_height: u16 = renderables.iter()
        .map(|r| r.desired_height(self.viewport.width()))
        .sum();
    
    // 3. 渲染
    let height = total_height.min(self.viewport.max_height());
    self.terminal.draw(height, |frame| {
        let mut y = 0u16;
        for renderable in &renderables {
            let h = renderable.desired_height(frame.area().width).min(height.saturating_sub(y));
            let area = Rect::new(0, y, frame.area().width, h);
            renderable.render(area, frame.buffer_mut());
            y += h;
        }
    }).ok();
}
```

### 3.2 Cargo.toml 依赖

```toml
[features]
default = []
tui = ["ratatui", "crossterm", "crossterm/event-stream"]

[dependencies]
ratatui = { version = "0.28", optional = true, features = ["crossterm"] }
crossterm = { version = "0.28", optional = true, features = ["event-stream", "bracketed-paste"] }
```

---

## 4. 关键测试

| 测试 | 文件 | 测试内容 |
|------|------|----------|
| `test_renderable_layout` | `render.rs` | ColumnRenderable 垂直布局正确 |
| `test_flex_renderable_distribution` | `render.rs` | FlexRenderable 比例分配正确 |
| `test_inset_renderable` | `render.rs` | 内边距渲染正确 |
| `test_history_cell_user_message` | `history_cell.rs` | 用户消息渲染正确 |
| `test_history_cell_assistant_message` | `history_cell.rs` | AI 回复渲染正确 |
| `test_diff_additions_removals` | `diff.rs` | 差异渲染正确着色 |
| `test_spinner_frame_cycle` | `spinner.rs` | Spinner 帧循环正确 |
| `test_streaming_cell_append` | `streaming.rs` | 流式追加正确 |

---

## 5. 实现顺序

### Step 1: 添加 ratatui 依赖
1. 更新 `Cargo.toml`，添加 ratatui
2. 验证编译通过

### Step 2: Renderable trait + 布局组件
1. 实现 `Renderable` trait
2. 实现 `ColumnRenderable`
3. 实现 `FlexRenderable`
4. 实现 `InsetRenderable`

### Step 3: 渲染管线升级
1. 创建 `TuiTerminal` 结构体
2. 实现 `draw()` 方法
3. 集成 SynchronizedUpdate

### Step 4: 状态指示器
1. 实现 `AiStatus` 枚举
2. 实现 `StatusBar` 渲染
3. 集成 Spinner 动画

### Step 5: 历史 Cell 渲染
1. 实现 `HistoryCell` 类型
2. 实现各类型的渲染
3. 集成到 App 渲染流程

### Step 6: 差异渲染
1. 实现 `render_diff()` 函数
2. 实现 `DiffPreview` 组件

### Step 7: Spinner 适配
1. 复用现有帧动画
2. 实现 `tick()` 和 `current_frame()`

---

## 6. 交付标准

- [x] `tui/render.rs` Renderable trait + 布局组件
- [x] `tui/terminal.rs` 渲染管线（draw_with_size + SynchronizedUpdate）
- [x] `tui/viewport.rs` Resize 处理
- [x] `tui/status.rs` 状态指示器 + Spinner 集成
- [x] `tui/history_cell.rs` 对话历史渲染
- [x] `tui/streaming.rs` 流式输出渲染
- [x] `tui/diff.rs` 差异渲染
- [x] `tui/spinner.rs` Spinner 动画适配
- [ ] 渲染管线正常工作
- [ ] 聊天区域可渲染文本和 markdown
- [ ] Resize 时 viewport 正确调整
- [ ] Status 行显示 AI 状态 + spinner 动画