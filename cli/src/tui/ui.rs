use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::tui::app::{AgentStatus, App, InputMode};

pub fn render(app: &App, frame: &mut Frame) {
    let size = frame.area();

    // Create layout with title, agent list, message history, and input box
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Min(0),    // Agent list
            Constraint::Length(5), // Message history
            Constraint::Length(3), // Input box
        ])
        .split(size);

    // Render title
    let title = Paragraph::new("Loom - Concurrent Agents Dashboard")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, chunks[0]);

    // Render agents
    render_agents(app, frame, chunks[1]);

    // Render message history
    render_messages(app, frame, chunks[2]);

    // Render input box
    render_input_box(app, frame, chunks[3]);
}

fn render_agents(app: &App, frame: &mut Frame, area: Rect) {
    if app.state.agent_order.is_empty() {
        let empty_msg = Paragraph::new("No agents running")
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title("Agents"));
        frame.render_widget(empty_msg, area);
        return;
    }

    // Create vertical layout for each agent
    let constraints: Vec<Constraint> = app
        .state
        .agent_order
        .iter()
        .map(|_| Constraint::Length(5))
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (idx, agent_id) in app.state.agent_order.iter().enumerate() {
        if let Some(agent) = app.state.agents.get(agent_id) {
            render_agent_card(agent, frame, chunks[idx]);
        }
    }
}

fn render_agent_card(agent: &crate::tui::app::AgentInfo, frame: &mut Frame, area: Rect) {
    let (status_color, status_text) = match agent.status {
        AgentStatus::Running => (Color::Yellow, "● Running"),
        AgentStatus::Completed => (Color::Green, "✓ Completed"),
        AgentStatus::Error => (Color::Red, "✗ Error"),
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(&agent.name, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" - "),
            Span::styled(status_text, Style::default().fg(status_color)),
        ]),
        Line::from(vec![
            Span::styled("Task: ", Style::default().fg(Color::Gray)),
            Span::raw(&agent.task),
        ]),
    ];

    if let Some(node) = &agent.current_node {
        lines.push(Line::from(vec![
            Span::styled("Node: ", Style::default().fg(Color::Gray)),
            Span::raw(node),
        ]));
    }

    if let Some(msg) = &agent.progress_message {
        lines.push(Line::from(vec![
            Span::styled("Progress: ", Style::default().fg(Color::Gray)),
            Span::raw(msg),
        ]));
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Agent: {}", agent.id))
                .border_style(Style::default().fg(status_color)),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

fn render_messages(app: &App, frame: &mut Frame, area: Rect) {
    let lines: Vec<Line> = app
        .state
        .messages
        .iter()
        .map(|msg| Line::from(Span::raw(msg)))
        .collect();

    let messages = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Messages")
                .border_style(Style::default().fg(Color::Blue)),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(messages, area);
}

fn render_input_box(app: &App, frame: &mut Frame, area: Rect) {
    let title = match app.input_mode {
        InputMode::Normal => "Input (press 'i' to edit, 'q' to quit)",
        InputMode::Editing => "Input (press 'Enter' to submit, 'Esc' to cancel)",
    };

    let input = Paragraph::new(app.input.as_str())
        .style(match app.input_mode {
            InputMode::Normal => Style::default().fg(Color::Gray),
            InputMode::Editing => Style::default().fg(Color::Yellow),
        })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(match app.input_mode {
                    InputMode::Normal => Style::default().fg(Color::Gray),
                    InputMode::Editing => Style::default().fg(Color::Green),
                }),
        );

    frame.render_widget(input, area);

    // Show cursor in editing mode
    if let InputMode::Editing = app.input_mode {
        frame.set_cursor_position((
            area.x + app.input.len() as u16 + 1,
            area.y + 1,
        ));
    }
}
