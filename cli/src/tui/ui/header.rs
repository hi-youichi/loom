use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::app::AppState;

pub struct Header;

impl Header {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::Rgb(100, 100, 120)));

        // Create header content with logo, stats, and time
        let agent_count = state.agents.len();
        let running_count = state.agents.values().filter(|a| a.status == crate::tui::app::AgentStatus::Running).count();
        let completed_count = state.agents.values().filter(|a| a.status == crate::tui::app::AgentStatus::Completed).count();

        let header_text = vec![
            Line::from(vec![
                Span::styled(
                    " ◈ Loom TUI ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("│ "),
                Span::styled(
                    "Agents: ",
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format!("{}", agent_count),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    "Running: ",
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format!("{}", running_count),
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    "Completed: ",
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format!("{}", completed_count),
                    Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                ),
            ]),
        ];

        let paragraph = Paragraph::new(header_text)
            .block(block)
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);
    }
}
