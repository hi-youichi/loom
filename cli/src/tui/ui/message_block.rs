//! Message block component for displaying messages

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Text,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::models::Message;

/// Widget for rendering a single message block
#[allow(dead_code)]
pub struct MessageBlock<'a> {
    message: &'a Message,
    is_collapsed: bool,
    is_selected: bool,
}

#[allow(dead_code)]
impl<'a> MessageBlock<'a> {
    pub fn new(message: &'a Message) -> Self {
        Self {
            message,
            is_collapsed: false,
            is_selected: false,
        }
    }

    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.is_collapsed = collapsed;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.is_selected = selected;
        self
    }

    /// Render the message block
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let border_style = if self.is_selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style);

        // Get message content
        let content = &self.message.content;
        let content_str = match content {
            crate::tui::models::MessageContent::Text(text) => text.clone(),
            _ => "Message".to_string(),
        };

        let text = if self.is_collapsed {
            // Show only first line
            let first_line = content_str.lines().next().unwrap_or("");
            Text::from(first_line)
        } else {
            // Show full content
            Text::from(content_str)
        };

        let paragraph = Paragraph::new(text).block(block);
        frame.render_widget(paragraph, area);
    }
}
