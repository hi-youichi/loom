//! Input box widget

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::tui::app::InputMode;

pub struct InputBox<'a> {
    content: &'a str,
    mode: InputMode,
    cursor_position: usize,
    history_count: usize,
    history_index: Option<usize>,
}

impl<'a> InputBox<'a> {
    pub fn new(content: &'a str, mode: InputMode) -> Self {
        Self {
            content,
            mode,
            cursor_position: 0,
            history_count: 0,
            history_index: None,
        }
    }

    pub fn cursor_position(mut self, pos: usize) -> Self {
        self.cursor_position = pos;
        self
    }

    pub fn history(mut self, count: usize, index: Option<usize>) -> Self {
        self.history_count = count;
        self.history_index = index;
        self
    }

    pub fn render(self, frame: &mut Frame, area: Rect) {
        let (title, border_color) = match self.mode {
            InputMode::Normal => ("Normal (Press 'i' to insert)", Color::Gray),
            InputMode::Editing => ("Insert (Press 'Esc' to exit)", Color::Green),
        };

        let mut title_spans = vec
![Span::styled(
            title,
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        )];

        // Show history info
        if self.history_count > 0 {
            let history_info = if let Some(idx) = self.history_index {
                format!(" [History: {}/{}]", idx + 1, self.history_count)
            } else {
                format!(" [History: {}]", self.history_count)
            };
            title_spans.push(Span::styled(
                history_info,
                Style::default().fg(Color::Yellow),
            ));
        }

        let block = Block::default()
            .title(Line::from(title_spans))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let paragraph = Paragraph::new(self.content)
            .block(block)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);

        // Show cursor in editing mode
        if self.mode == InputMode::Editing {
            let cursor_x = area.x + 1 + self.cursor_position as u16 % (area.width - 2);
            let cursor_y = area.y + 1 + self.cursor_position as u16 / (area.width - 2);

            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}
