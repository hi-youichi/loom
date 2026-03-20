//! Input Bar Component - Bottom input area for user text entry
//!
//! Renders the input bar with:
//! - Mode indicator (Normal/Editing)
//! - Input field with cursor
//! - Help text with keyboard shortcuts

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::app::InputMode;

/// Render the input bar at the bottom of the screen
pub fn render_input_bar(
    frame: &mut Frame,
    area: Rect,
    input: &str,
    input_mode: &InputMode,
    cursor_position: usize,
) {
    // Split into input field and help text
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    // Render input field
    let (title, style) = match input_mode {
        InputMode::Normal => (
            " Normal Mode (Press 'i' to enter input mode) ",
            Style::default(),
        ),
        InputMode::Editing => (
            " Input Mode (Press 'Esc' to exit, 'Enter' to submit) ",
            Style::default().fg(Color::Yellow),
        ),
    };

    let input_paragraph = Paragraph::new(input)
        .style(match input_mode {
            InputMode::Normal => Style::default(),
            InputMode::Editing => Style::default().fg(Color::Yellow),
        })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_style(style),
        );
    frame.render_widget(input_paragraph, chunks[0]);

    // Set cursor position in editing mode
    if *input_mode == InputMode::Editing {
        frame.set_cursor_position((
            chunks[0].x + cursor_position as u16 + 1,
            chunks[0].y + 1,
        ));
    }

    // Render help text
    let help_spans = match input_mode {
        InputMode::Normal => vec![
            Span::styled("'h/l'", Style::default().fg(Color::Cyan)),
            Span::raw(" switch panel  "),
            Span::styled("'j/k'", Style::default().fg(Color::Cyan)),
            Span::raw(" navigate  "),
            Span::styled("'i'", Style::default().fg(Color::Cyan)),
            Span::raw(" input  "),
            Span::styled("'q'", Style::default().fg(Color::Cyan)),
            Span::raw(" quit"),
        ],
        InputMode::Editing => vec![
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(" submit  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" cancel  "),
            Span::styled("Backspace", Style::default().fg(Color::Cyan)),
            Span::raw(" delete"),
        ],
    };

    let help_text = Line::from(help_spans);
    let help_paragraph = Paragraph::new(help_text)
        .alignment(Alignment::Center);
    frame.render_widget(help_paragraph, chunks[1]);
}
