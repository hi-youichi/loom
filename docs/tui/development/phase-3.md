# Phase 3: 交互系统实现方案

## 概述

Phase 3 实现 Loom TUI 的**交互系统**——用户输入、审批弹窗、选择列表、状态机。这是 TUI 的核心体验层，让用户可以与 AI 进行交互式对话。

**目标**：用户可输入文本并提交，AI 可请求审批，用户可交互式响应，中断流程正常。

**预计文件**：10-12 个文件，~2000 行代码

---

## 1. 文件清单

| 文件 | 职责 | 预计行数 | 依赖 |
|------|------|----------|------|
| `tui/pane.rs` | PaneView trait + PaneStack | ~150 | 无 |
| `tui/composer.rs` | 输入框 | ~400 | ratatui |
| `tui/approval.rs` | 审批弹窗 | ~250 | ratatui |
| `tui/selection.rs` | 选择列表 | ~200 | ratatui |
| `tui/state.rs` | 应用状态机 + 输入状态机 | ~200 | 无 |
| `tui/pane/mod.rs` | 面板子模块入口 | ~30 | 无 |
| `tui/pane/composer_pane.rs` | 输入框面板 | ~150 | pane.rs, composer.rs |
| `tui/pane/approval_pane.rs` | 审批面板 | ~150 | pane.rs, approval.rs |
| `tui/pane/selection_pane.rs` | 选择面板 | ~150 | pane.rs, selection.rs |
| `tui/pane/feedback_pane.rs` | 反馈面板 | ~100 | pane.rs |

---

## 2. 核心实现

### 2.1 PaneView Trait 与 PaneStack (`tui/pane.rs`)

#### 2.1.1 PaneView Trait

```rust
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::SetCursorStyle,
};
use crossterm::event::KeyEvent;

/// 按键处理结果
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Handled {
    /// 事件已处理，不继续传递
    Handled,
    /// 事件未处理，传递给下一层
    NotHandled,
}

/// Ctrl+C 处理结果
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CtrlCAction {
    /// 未处理，继续传递
    NotHandled,
    /// 已处理，但继续运行
    Handled,
    /// 取消当前操作
    Cancel,
}

/// 面板视图接口
pub trait PaneView: Renderable {
    /// 处理按键事件
    fn handle_key_event(&mut self, key: KeyEvent) -> Handled;

    /// 视图是否已完成（完成后自动 pop）
    fn is_complete(&self) -> bool {
        false
    }

    /// Ctrl+C 处理
    fn on_ctrl_c(&mut self) -> CtrlCAction {
        CtrlCAction::NotHandled
    }

    /// 视图唯一标识
    fn view_id(&self) -> Option<&'static str> {
        None
    }
}
```

#### 2.1.2 PaneStack

```rust
/// 栈式面板管理器
pub struct PaneStack {
    /// 面板栈，栈顶为当前活跃面板
    stack: Vec<Box<dyn PaneView>>,
    /// 基座面板（始终存在，通常是 ChatComposer）
    base: Option<Box<dyn PaneView>>,
}

impl PaneStack {
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            base: None,
        }
    }

    /// 设置基座面板
    pub fn set_base(&mut self, base: Box<dyn PaneView>) {
        self.base = Some(base);
    }

    /// 推入新面板（栈顶）
    pub fn push(&mut self, pane: Box<dyn PaneView>) {
        self.stack.push(pane);
    }

    /// 弹出栈顶面板
    pub fn pop(&mut self) -> Option<Box<dyn PaneView>> {
        self.stack.pop()
    }

    /// 获取当前活跃面板（栈顶优先）
    fn active(&mut self) -> Option<&mut Box<dyn PaneView>> {
        if !self.stack.is_empty() {
            Some(&mut self.stack[self.stack.len() - 1])
        } else {
            self.base.as_mut()
        }
    }

    /// 处理按键事件
    pub fn handle_key_event(&mut self, key: KeyEvent) -> Handled {
        // 1. 先让栈顶面板处理
        if let Some(active) = self.active() {
            if active.handle_key_event(key) == Handled::Handled {
                // 检查栈顶面板是否已完成
                self.cleanup_completed();
                return Handled::Handled;
            }
        }
        Handled::NotHandled
    }

    /// 清理已完成的栈顶面板
    fn cleanup_completed(&mut self) {
        while let Some(top) = self.stack.last() {
            if top.is_complete() {
                self.stack.pop();
            } else {
                break;
            }
        }
    }

    /// 获取面板栈大小
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// 当前是否有活跃面板
    pub fn is_active(&self) -> bool {
        self.stack.is_empty() && self.base.is_none()
    }
}

impl Renderable for PaneStack {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if let Some(top) = self.stack.last() {
            top.render(area, buf);
        } else if let Some(base) = &self.base {
            base.render(area, buf);
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        if let Some(top) = self.stack.last() {
            top.desired_height(width)
        } else if let Some(base) = &self.base {
            base.desired_height(width)
        } else {
            0
        }
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        if let Some(top) = self.stack.last() {
            top.cursor_pos(area)
        } else if let Some(base) = &self.base {
            base.cursor_pos(area)
        } else {
            None
        }
    }

    fn cursor_style(&self, area: Rect) -> SetCursorStyle {
        if let Some(top) = self.stack.last() {
            top.cursor_style(area)
        } else if let Some(base) = &self.base {
            base.cursor_style(area)
        } else {
            SetCursorStyle::DefaultUserShape
        }
    }
}
```

### 2.2 输入框 (`tui/composer.rs`)

#### 2.2.1 设计原则

- 使用 ratatui 的 `TextArea`（或自定义实现）作为输入组件
- 支持多行输入（Shift+Enter 换行，Enter 提交）
- 支持 slash 命令（`/` 开头触发命令补全）
- 支持输入历史（↑/↓ 浏览历史）
- 支持粘贴内容（通过 bracketed paste 协议）

#### 2.2.2 核心结构

```rust
/// 输入框组件
pub struct Composer {
    /// 当前输入内容
    input: String,
    /// 光标位置（字符索引）
    cursor: usize,
    /// 输入历史
    history: Vec<String>,
    /// 历史浏览位置（None 表示新输入）
    history_index: Option<usize>,
    /// 是否正在输入斜杠命令
    slash_command: bool,
    /// 输入提示文本
    placeholder: String,
}

impl Composer {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            slash_command: false,
            placeholder: " 输入消息... (/help 查看命令)".to_string(),
        }
    }

    /// 获取当前输入内容
    pub fn content(&self) -> &str {
        &self.input
    }

    /// 提交当前输入
    pub fn submit(&mut self) -> String {
        let content = self.input.clone();
        if !content.is_empty() {
            self.history.push(content.clone());
            self.history_index = None;
        }
        self.input.clear();
        self.cursor = 0;
        self.slash_command = false;
        content
    }

    /// 插入文本（从粘贴或自动补全）
    pub fn insert_text(&mut self, text: &str) {
        let before = &self.input[..self.cursor];
        let after = &self.input[self.cursor..];
        self.input = format!("{}{}{}", before, text, after);
        self.cursor += text.len();
        self.update_slash_state();
    }

    /// 删除光标前一个字符
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let before = &self.input[..self.cursor - 1];
            let after = &self.input[self.cursor..];
            self.input = format!("{}{}", before, after);
            self.cursor -= 1;
            self.update_slash_state();
        }
    }

    /// 处理按键事件
    pub fn handle_key(&mut self, key: KeyEvent) -> ComposerAction {
        match key.code {
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift+Enter: 换行
                    self.insert_text("\n");
                    ComposerAction::Continue
                } else {
                    // Enter: 提交
                    if self.input.trim().is_empty() {
                        ComposerAction::Continue
                    } else {
                        let content = self.submit();
                        ComposerAction::Submit(content)
                    }
                }
            }
            KeyCode::Up => {
                self.navigate_history(-1);
                ComposerAction::Continue
            }
            KeyCode::Down => {
                self.navigate_history(1);
                ComposerAction::Continue
            }
            KeyCode::Char(c) => {
                self.insert_text(&c.to_string());
                ComposerAction::Continue
            }
            KeyCode::Backspace => {
                self.backspace();
                ComposerAction::Continue
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                ComposerAction::Continue
            }
            KeyCode::Right => {
                if self.cursor < self.input.len() {
                    self.cursor += 1;
                }
                ComposerAction::Continue
            }
            KeyCode::Home => {
                self.cursor = 0;
                ComposerAction::Continue
            }
            KeyCode::End => {
                self.cursor = self.input.len();
                ComposerAction::Continue
            }
            KeyCode::Tab => {
                // Tab: 自动补全（后续实现）
                ComposerAction::Continue
            }
            _ => ComposerAction::Continue,
        }
    }

    fn navigate_history(&mut self, direction: i32) {
        if self.history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                if direction < 0 {
                    self.history_index = Some(self.history.len() - 1);
                    self.input = self.history[self.history.len() - 1].clone();
                    self.cursor = self.input.len();
                }
            }
            Some(idx) => {
                let new_idx = if direction < 0 {
                    if idx > 0 { idx - 1 } else { 0 }
                } else {
                    if idx + 1 < self.history.len() { idx + 1 } else { return; }
                };
                self.history_index = Some(new_idx);
                self.input = self.history[new_idx].clone();
                self.cursor = self.input.len();
            }
        }
    }

    fn update_slash_state(&mut self) {
        self.slash_command = self.input.starts_with('/') && !self.input.contains(' ');
    }
}

/// 输入框操作结果
#[derive(Debug)]
pub enum ComposerAction {
    /// 继续输入
    Continue,
    /// 提交内容
    Submit(String),
}
```

#### 2.2.3 渲染实现

```rust
impl Renderable for Composer {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        // 输入框区域
        let block = Block::default()
            .title(" 输入 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        // 显示内容
        let display_text = if self.input.is_empty() {
            self.placeholder.clone()
        } else {
            self.input.clone()
        };

        let paragraph = Paragraph::new(display_text)
            .block(block)
            .wrap(Wrap { trim: false });

        if self.input.is_empty() {
            paragraph.style(Style::default().fg(Color::DarkGray));
        }

        paragraph.render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let line_count = if self.input.is_empty() {
            1
        } else {
            self.input.lines().count() as u16
        };
        // Min 3 lines (1 input + 2 borders), expand with content
        line_count.max(1) + 2
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        // 计算光标在输入框中的位置
        let line_before_cursor = &self.input[..self.cursor];
        let lines_before = line_before_cursor.lines().count().saturating_sub(1);
        let last_line_start = if lines_before > 0 {
            line_before_cursor.rfind('\n')
                .map(|pos| pos + 1)
                .unwrap_or(0)
        } else {
            0
        };
        let col = (self.cursor - last_line_start) as u16;
        let row = lines_before as u16;

        Some((area.x + 1 + col, area.y + 1 + row))
    }

    fn cursor_style(&self, _area: Rect) -> SetCursorStyle {
        SetCursorStyle::BlinkingBar
    }
}
```

### 2.3 审批弹窗 (`tui/approval.rs`)

#### 2.3.1 设计原则

- 当 AI 需要执行文件修改或命令时，弹出审批视图
- 支持 Y/N/D/A 快捷键
- 显示审批上下文（将要执行的命令或修改）
- 支持设置"始终允许"选项

#### 2.3.2 核心结构

```rust
/// 审批请求类型
#[derive(Debug, Clone)]
pub enum ApprovalRequest {
    /// 命令执行
    Command {
        command: String,
        description: String,
    },
    /// 文件修改
    FileEdit {
        file_path: String,
        diff: String,
    },
    /// 工具调用
    ToolCall {
        tool_name: String,
        args: serde_json::Value,
    },
}

/// 审批结果
#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalResult {
    /// 允许本次
    Allow,
    /// 拒绝
    Deny,
    /// 始终允许（会话内）
    AlwaysAllow,
    /// 显示差异
    ShowDiff,
}

/// 审批弹窗
pub struct ApprovalOverlay {
    /// 审批请求
    request: ApprovalRequest,
    /// 处理结果
    result: Option<ApprovalResult>,
    /// 是否显示差异
    show_diff: bool,
    /// 错误消息（如果有）
    error: Option<String>,
}

impl ApprovalOverlay {
    pub fn new(request: ApprovalRequest) -> Self {
        Self {
            request,
            result: None,
            show_diff: false,
            error: None,
        }
    }

    pub fn result(&self) -> Option<ApprovalResult> {
        self.result
    }
}

impl PaneView for ApprovalOverlay {
    fn handle_key_event(&mut self, key: KeyEvent) -> Handled {
        if let KeyCode::Char(c) = key.code {
            match c.to_ascii_lowercase() {
                'y' => {
                    self.result = Some(ApprovalResult::Allow);
                    Handled::Handled
                }
                'n' | '\x1b' => {
                    self.result = Some(ApprovalResult::Deny);
                    Handled::Handled
                }
                'a' => {
                    self.result = Some(ApprovalResult::AlwaysAllow);
                    Handled::Handled
                }
                'd' => {
                    self.show_diff = !self.show_diff;
                    Handled::Handled
                }
                _ => Handled::NotHandled,
            }
        } else {
            Handled::NotHandled
        }
    }

    fn is_complete(&self) -> bool {
        self.result.is_some()
    }

    fn on_ctrl_c(&mut self) -> CtrlCAction {
        self.result = Some(ApprovalResult::Deny);
        CtrlCAction::Handled
    }

    fn view_id(&self) -> Option<&'static str> {
        Some("approval")
    }
}

impl Renderable for ApprovalOverlay {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(" 审批 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );

        // 渲染请求内容
        let request_text = match &self.request {
            ApprovalRequest::Command { command, description } => {
                format!("{}\n\n命令: {}", description, command)
            }
            ApprovalRequest::FileEdit { file_path, diff } => {
                format!("文件: {}\n\n{}", file_path, diff)
            }
            ApprovalRequest::ToolCall { tool_name, args } => {
                format!("工具: {}\n参数: {}", tool_name, args)
            }
        };

        let paragraph = Paragraph::new(request_text)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);

        // 底部提示
        let hint_text = " [Y] 允许  [N] 拒绝  [D] 显示差异  [A] 始终允许  [Esc] 取消 ";
        let hint = Paragraph::new(hint_text)
            .style(Style::default().fg(Color::DarkGray));
        let hint_area = Rect::new(
            area.x, area.y + area.height - 1,
            area.width, 1,
        );
        hint.render(hint_area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let content_lines = match &self.request {
            ApprovalRequest::Command { command, description } => {
                description.lines().count() + command.lines().count() + 2
            }
            ApprovalRequest::FileEdit { file_path, diff } => {
                1 + diff.lines().count() + 1
            }
            ApprovalRequest::ToolCall { .. } => 3,
        };
        (content_lines as u16 + 4).min(20) // 最多 20 行
    }
}
```

### 2.4 选择列表 (`tui/selection.rs`)

```rust
/// 选择列表项
pub struct SelectionItem {
    pub label: String,
    pub description: Option<String>,
    pub value: String,
}

/// 通用选择列表
pub struct SelectionList {
    /// 列表项
    items: Vec<SelectionItem>,
    /// 当前选中索引
    selected: usize,
    /// 搜索过滤文本
    filter: String,
    /// 是否已选择
    confirmed: bool,
    /// 选择结果
    result: Option<String>,
    /// 标题
    title: String,
}

impl SelectionList {
    pub fn new(items: Vec<SelectionItem>, title: String) -> Self {
        Self {
            items,
            selected: 0,
            filter: String::new(),
            confirmed: false,
            result: None,
            title,
        }
    }

    pub fn result(&self) -> Option<&str> {
        self.result.as_deref()
    }
}

impl PaneView for SelectionList {
    fn handle_key_event(&mut self, key: KeyEvent) -> Handled {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                Handled::Handled
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.visible_items().len() {
                    self.selected += 1;
                }
                Handled::Handled
            }
            KeyCode::Enter => {
                if let Some(item) = self.visible_items().get(self.selected) {
                    self.result = Some(item.value.clone());
                    self.confirmed = true;
                }
                Handled::Handled
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.selected = 0;
                Handled::Handled
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.selected = 0;
                Handled::Handled
            }
            KeyCode::Esc => {
                self.confirmed = true;
                Handled::Handled
            }
            _ => Handled::NotHandled,
        }
    }

    fn is_complete(&self) -> bool {
        self.confirmed
    }

    fn on_ctrl_c(&mut self) -> CtrlCAction {
        self.confirmed = true;
        CtrlCAction::Handled
    }

    fn view_id(&self) -> Option<&'static str> {
        Some("selection")
    }

    fn visible_items(&self) -> Vec<&SelectionItem> {
        if self.filter.is_empty() {
            self.items.iter().collect()
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.items.iter()
                .filter(|item| item.label.to_lowercase().contains(&filter_lower))
                .collect()
        }
    }
}

impl Renderable for SelectionList {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        block.render(area, buf);

        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );

        let visible = self.visible_items();
        let start = self.selected.saturating_sub(inner.height as usize - 1);

        for (i, item) in visible.iter().enumerate().skip(start) {
            let y = inner.y + (i - start) as u16;
            if y >= inner.y + inner.height {
                break;
            }

            let is_selected = i == self.selected;
            let style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };

            let label = if let Some(desc) = &item.description {
                format!(" {} - {}", item.label, desc)
            } else {
                format!(" {}", item.label)
            };

            let span = Span::styled(label, style);
            span.render(Rect::new(inner.x, y, inner.width, 1), buf);
        }

        // 搜索提示
        if !self.filter.is_empty() {
            let filter_text = format!(" 过滤: {}", self.filter);
            let filter_span = Span::styled(
                filter_text,
                Style::default().fg(Color::Yellow),
            );
            filter_span.render(Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1), buf);
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        let visible_count = self.visible_items().len() as u16;
        (visible_count + 2).min(15) // 最多 15 行
    }
}
```

### 2.5 状态机 (`tui/state.rs`)

#### 2.5.1 应用状态机

```rust
/// 应用全局状态
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppState {
    /// 空闲，等待用户输入
    Idle,
    /// 用户正在输入
    Inputting,
    /// 提交中
    Submitting,
    /// AI 正在思考/执行
    Processing,
    /// 等待审批
    AwaitingApproval,
    /// 中断
    Interrupted,
    /// 错误
    Error,
    /// 退出中
    Exiting,
}

/// 状态转换
impl AppState {
    pub fn transition(&self, event: AppEvent) -> Result<AppState, StateError> {
        match (self, event) {
            // Idle → Inputting (用户开始输入)
            (AppState::Idle, AppEvent::StartInput) => Ok(AppState::Inputting),
            // Inputting → Submitting (用户提交)
            (AppState::Inputting, AppEvent::Submit) => Ok(AppState::Submitting),
            // Submitting → Processing (AI 开始处理)
            (AppState::Submitting, AppEvent::Processing) => Ok(AppState::Processing),
            // Processing → Idle (AI 完成)
            (AppState::Processing, AppEvent::Completed) => Ok(AppState::Idle),
            // Processing → AwaitingApproval (AI 请求审批)
            (AppState::Processing, AppEvent::RequestApproval) => Ok(AppState::AwaitingApproval),
            // AwaitingApproval → Processing (用户审批完成)
            (AppState::AwaitingApproval, AppEvent::ApprovalDone) => Ok(AppState::Processing),
            // 任何状态 → Interrupted (Ctrl+C)
            (_, AppEvent::Interrupt) => Ok(AppState::Interrupted),
            // Interrupted → Idle (恢复)
            (AppState::Interrupted, AppEvent::Resume) => Ok(AppState::Idle),
            // 任何状态 → Error
            (_, AppEvent::Error) => Ok(AppState::Error),
            // Error → Idle (恢复)
            (AppState::Error, AppEvent::Resume) => Ok(AppState::Idle),
            // 任何状态 → Exiting
            (_, AppEvent::Exit) => Ok(AppState::Exiting),
            // 不允许的转换
            _ => Err(StateError(format!(
                "不能从 {:?} 转换到 {:?}",
                self, event
            ))),
        }
    }
}

/// 应用事件
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppEvent {
    StartInput,
    Submit,
    Processing,
    Completed,
    RequestApproval,
    ApprovalDone,
    Interrupt,
    Resume,
    Error,
    Exit,
}

#[derive(Debug, Clone)]
pub struct StateError(pub String);
```

---

## 3. 关键交互流程

### 3.1 用户输入流程

```
用户按 Enter
  → Composer.handle_key(Enter)
  → ComposerAction::Submit(content)
  → App::handle_submit(content)
  → 状态转换: Inputting → Submitting
  → 发送消息到 Agent
  → 状态转换: Submitting → Processing
  → 创建 StreamingCell
  → 等待 Agent 事件
```

### 3.2 审批流程

```
AI 请求审批
  → App::handle_agent_event(ApprovalNeeded)
  → 状态转换: Processing → AwaitingApproval
  → PaneStack::push(ApprovalOverlay)
  → 用户按 Y/N/A
  → ApprovalOverlay::handle_key_event()
  → ApprovalOverlay::is_complete() = true
  → PaneStack::cleanup_completed() → pop
  → 状态转换: AwaitingApproval → Processing
  → 发送审批结果到 Agent
```

### 3.3 中断流程

```
用户按 Ctrl+C
  → App::handle_ctrl_c()
  → 状态转换: * → Interrupted
  → 发送中断信号到 Agent
  → 显示中断提示
  → 用户按任意键恢复
  → 状态转换: Interrupted → Idle
```

---

## 4. 集成点

### 4.1 App 集成

```rust
// tui/app.rs
impl App {
    fn handle_key(&mut self, key: KeyEvent) {
        match self.state {
            AppState::Idle | AppState::Inputting => {
                // 先让 PaneStack 处理
                if self.pane_stack.handle_key_event(key) == Handled::NotHandled {
                    // 如果 PaneStack 未处理，进入编辑模式
                    self.state = AppState::Inputting;
                }
            }
            AppState::Processing => {
                // 处理中，只响应 Ctrl+C
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.interrupt();
                }
            }
            AppState::AwaitingApproval => {
                // 让审批面板处理
                self.pane_stack.handle_key_event(key);
            }
            AppState::Interrupted => {
                // 任意键恢复
                self.state = self.state.transition(AppEvent::Resume).unwrap();
            }
            _ => {}
        }
    }

    fn handle_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta(content) => {
                if let Some(active) = &mut self.active_cell {
                    active.append_text(&content);
                }
            }
            AgentEvent::ToolCall(tool_call) => {
                // 根据策略决定是否审批
                if self.approval_strategy == ApprovalStrategy::Always {
                    // 直接执行
                } else {
                    // 弹出审批
                    let request = ApprovalRequest::ToolCall {
                        tool_name: tool_call.name,
                        args: tool_call.arguments,
                    };
                    self.pane_stack.push(Box::new(ApprovalOverlay::new(request)));
                    self.state = self.state.transition(AppEvent::RequestApproval).unwrap();
                }
            }
            AgentEvent::Completed => {
                // 完成回复
                if let Some(active) = self.active_cell.take() {
                    self.history.push(HistoryCell::AssistantMessage {
                        content: active.content,
                        timestamp: chrono::Utc::now(),
                    });
                }
                self.state = self.state.transition(AppEvent::Completed).unwrap();
            }
            AgentEvent::Error(e) => {
                self.error = Some(e);
                self.state = self.state.transition(AppEvent::Error).unwrap();
            }
            _ => {}
        }
    }
}
```

---

## 5. 实现顺序

### Step 1: PaneView trait + PaneStack
1. 实现 `PaneView` trait
2. 实现 `Handled`、`CtrlCAction` 枚举
3. 实现 `PaneStack` 结构体

### Step 2: Composer 输入框
1. 实现 `Composer` 结构体
2. 实现 `handle_key()` 方法
3. 实现 `Renderable` trait
4. 实现 `ComposerPane` 面板包装

### Step 3: Approval 审批弹窗
1. 实现 `ApprovalRequest` 枚举
2. 实现 `ApprovalOverlay` 结构体
3. 实现 `PaneView` + `Renderable`

### Step 4: Selection 选择列表
1. 实现 `SelectionItem` 结构体
2. 实现 `SelectionList` 结构体
3. 实现 `PaneView` + `Renderable`

### Step 5: 状态机
1. 实现 `AppState` 枚举
2. 实现 `AppEvent` 枚举
3. 实现 `transition()` 状态转换

### Step 6: 集成到 App
1. 将 PaneStack 集成到 App 主循环
2. 实现按键分发逻辑
3. 实现 Agent 事件处理

---

## 6. 交付标准

- [x] `tui/pane.rs` PaneView trait + PaneStack
- [x] `tui/composer.rs` 输入框
- [x] `tui/approval.rs` 审批弹窗
- [x] `tui/selection.rs` 选择列表
- [x] `tui/state.rs` 状态机
- [x] `tui/pane/mod.rs` 面板子模块入口
- [x] `tui/pane/composer_pane.rs` 输入框面板
- [x] `tui/pane/approval_pane.rs` 审批面板
- [x] `tui/pane/selection_pane.rs` 选择面板
- [ ] 用户可输入文本并提交
- [ ] AI 可请求审批，用户可交互式响应
- [ ] 底部面板栈正常工作
- [ ] 状态机状态转换正确
- [ ] 中断流程（Ctrl+C）正常工作