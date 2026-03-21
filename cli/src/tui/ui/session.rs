//! Session 面板组件 - 显示会话历史和消息列表

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect, Alignment},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::tui::{InputMode, models::{Message, MessageRole, MessageContent, ToolCallStatus}};

/// Session 面板组件
pub struct SessionPanel {
    /// 当前选中的 Session ID
    selected_session: Option<String>,
    /// 滚动偏移
    scroll_offset: usize,
    /// 是否显示详细信息
    show_details: bool,
}

impl SessionPanel {
    pub fn new() -> Self {
        Self {
            selected_session: None,
            scroll_offset: 0,
            show_details: false,
        }
    }

    /// 渲染 Session 面板
    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        messages: &[Message],
        input_mode: &InputMode,
    ) {
        // 创建布局：标题 + 消息列表
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        // 渲染标题栏
        self.render_header(frame, chunks[0], messages);

        // 渲染消息列表
        self.render_messages(frame, chunks[1], messages);
    }

    /// 渲染标题栏
    fn render_header(&self, frame: &mut Frame, area: Rect, messages: &[Message]) {
        let title = if let Some(session_id) = &self.selected_session {
            format!(" Session: {} ", session_id)
        } else {
            " Session History ".to_string()
        };

        let stats = format!("({} messages)", messages.len());

        let header = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(ratatui::layout::Alignment::Left)
            .style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(stats)
            .style(Style::default().fg(Color::Gray))
            .block(header);

        frame.render_widget(paragraph, area);
    }

    /// 渲染消息列表
    fn render_messages(&mut self, frame: &mut Frame, area: Rect, messages: &[Message]) {
        if messages.is_empty() {
            let empty_msg = Paragraph::new("No messages yet...")
                .style(Style::default().fg(Color::Gray))
                .alignment(ratatui::layout::Alignment::Center)
                .block(Block::default().borders(Borders::ALL));
            frame.render_widget(empty_msg, area);
            return;
        }

        // 将消息转换为显示项
        let items: Vec<ListItem> = messages
            .iter()
            .flat_map(|msg| self.message_to_list_items(msg))
            .collect();

        // 创建列表 widget
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray));

        // 渲染列表
        frame.render_widget(list, area);

        // TODO: 添加滚动条
    }

    /// 将消息转换为 ListItem
    fn message_to_list_items(&self, message: &Message) -> Vec<ListItem> {
        let mut items = Vec::new();

        // 消息头部
        let role_icon = match message.role {
            MessageRole::User => "👤",
            MessageRole::Assistant => "🤖",
            MessageRole::System => "⚙️",
        };

        let role_name = match message.role {
            MessageRole::User => "User",
            MessageRole::Assistant => "Assistant",
            MessageRole::System => "System",
        };

        let header_style = match message.role {
            MessageRole::User => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            MessageRole::Assistant => Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
            MessageRole::System => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        };

        // 时间戳
        let timestamp = message.timestamp.format("%H:%M:%S");

        // 消息头部行
        let header = Line::from(vec![
            Span::styled(format!("{} ", role_icon), header_style),
            Span::styled(role_name, header_style),
            Span::raw(" "),
            Span::styled(timestamp.to_string(), Style::default().fg(Color::Gray)),
        ]);

        items.push(ListItem::new(header));

        // 消息内容
        match &message.content {
            MessageContent::Text(text) => {
                // 简单的文本换行
                for line in text.lines() {
                    let content_line = Line::from(Span::raw(format!("  {}", line)));
                    items.push(ListItem::new(content_line));
                }
            }
            MessageContent::Thinking { content, .. } => {
                // 思考过程
                let thinking_header = Line::from(Span::styled(
                    "  💭 Thinking...",
                    Style::default().fg(Color::Magenta).add_modifier(Modifier::ITALIC),
                ));
                items.push(ListItem::new(thinking_header));

                for line in content.lines() {
                    let thinking_line = Line::from(Span::styled(
                        format!("    {}", line),
                        Style::default().fg(Color::DarkGray),
                    ));
                    items.push(ListItem::new(thinking_line));
                }
            }
            MessageContent::ToolCall { name, arguments, status, .. } => {
                // 工具调用
                let status_icon = match status {
                    ToolCallStatus::Pending => "⏳",
                    ToolCallStatus::Executing => "🔄",
                    ToolCallStatus::Success => "✅",
                    ToolCallStatus::Error => "❌",
                };
                let tool_header = Line::from(vec![
                    Span::styled("  🔧 ", Style::default().fg(Color::Yellow)),
                    Span::styled(name.clone(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                    Span::styled(status_icon, Style::default().fg(Color::Yellow)),
                ]);
                items.push(ListItem::new(tool_header));

                // 参数（如果显示详细信息）
                if self.show_details {
                    let args_line = Line::from(Span::styled(
                        format!("    Args: {}", arguments),
                        Style::default().fg(Color::Gray),
                    ));
                    items.push(ListItem::new(args_line));
                }
            }
            MessageContent::ToolResult { tool_call_id, content: result_content, success } => {
                let icon = if *success { "✅" } else { "❌" };
                let color = if *success { Color::Green } else { Color::Red };
                let result_line = Line::from(Span::styled(
                    format!("  {} Tool Result ({}): {}", icon, tool_call_id, result_content),
                    Style::default().fg(color),
                ));
                items.push(ListItem::new(result_line));
            }
            MessageContent::Composite(blocks) => {
                for block in blocks {
                    let block_line = Line::from(Span::raw(format!("  {}", block.content)));
                    items.push(ListItem::new(block_line));
                }
            }
        }

        // 空行分隔
        items.push(ListItem::new(Line::from("")));

        items
    }

    /// 向下滚动
    pub fn scroll_down(&mut self, messages: &[Message]) {
        // TODO: 实现虚拟滚动
    }

    /// 向上滚动
    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    /// 切换详细信息显示
    pub fn toggle_details(&mut self) {
        self.show_details = !self.show_details;
    }

    /// 选择 Session
    pub fn select_session(&mut self, session_id: String) {
        self.selected_session = Some(session_id);
        self.scroll_offset = 0;
    }

    /// 清除选择
    pub fn clear_selection(&mut self) {
        self.selected_session = None;
        self.scroll_offset = 0;
    }
}

impl Default for SessionPanel {
    fn default() -> Self {
        Self::new()
    }
}
