//! Agent card widget

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::tui::{AgentInfo, AgentStatus};

pub struct AgentCard<'a> {
    agent: &'a AgentInfo,
    selected: bool,
}

impl<'a> AgentCard<'a> {
    pub fn new(agent: &'a AgentInfo) -> Self {
        Self {
            agent,
            selected: false,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn render(self, frame: &mut Frame, area: Rect) {
        let border_style = if self.selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let status_color = match self.agent.status {
            AgentStatus::Running => Color::Green,
            AgentStatus::Completed => Color::Blue,
            AgentStatus::Error => Color::Red,
        };

        let status_text = match self.agent.status {
            AgentStatus::Running => "● Running",
            AgentStatus::Completed => "✓ Completed",
            AgentStatus::Error => "✗ Error",
        };

        let title = Line::from(vec![
            Span::styled(&self.agent.name, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(status_text, Style::default().fg(status_color)),
        ]);

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);

        let mut lines = vec![
            Line::from(Span::styled(
                "Task:",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::raw(&self.agent.task)),
        ];

        if let Some(node) = &self.agent.current_node {
            lines.push(Line::from(Span::styled(
                format!("Node: {}", node),
                Style::default().fg(Color::Cyan),
            )));
        }

        if let Some(msg) = &self.agent.progress_message {
            lines.push(Line::from(Span::styled(
                msg,
                Style::default().fg(Color::Yellow),
            )));
        }

        if let Some(result) = &self.agent.result {
            lines.push(Line::from(Span::styled(
                "Result:",
                Style::default().fg(Color::Green),
            )));
            lines.push(Line::from(Span::raw(result)));
        }

        if let Some(error) = &self.agent.error {
            lines.push(Line::from(Span::styled(
                "Error:",
                Style::default().fg(Color::Red),
            )));
            lines.push(Line::from(Span::raw(error)));
        }

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }
}
