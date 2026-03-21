//! Agent Grid Component - Displays a grid of agent cards

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::app::{AgentInfo, AgentStatus};

/// Render the agent grid
pub fn render_agents_grid(
    agents: &[AgentInfo],
    area: Rect,
    frame: &mut Frame,
    selected_index: Option<usize>,
) {
    if agents.is_empty() {
        // Show empty state
        let empty = Paragraph::new("No agents running")
            .style(Style::default().fg(Color::Gray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Agents")
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
        frame.render_widget(empty, area);
        return;
    }

    // Calculate grid dimensions
    let cols = calculate_columns(area.width, agents.len());
    let rows = (agents.len() + cols - 1) / cols;

    // Create row layouts
    let row_constraints: Vec<Constraint> = (0..rows)
        .map(|_| Constraint::Ratio(1, rows as u32))
        .collect();
    
    let row_layouts = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(area);

    // Render each row
    for (row_idx, row_area) in row_layouts.iter().enumerate() {
        let col_constraints: Vec<Constraint> = (0..cols)
            .map(|_| Constraint::Ratio(1, cols as u32))
            .collect();
        
        let col_layouts = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints)
            .split(*row_area);

        // Render each cell in the row
        for (col_idx, cell_area) in col_layouts.iter().enumerate() {
            let agent_idx = row_idx * cols + col_idx;
            if agent_idx < agents.len() {
                let is_selected = selected_index == Some(agent_idx);
                render_agent_card(&agents[agent_idx], *cell_area, frame, is_selected);
            }
        }
    }
}

/// Calculate optimal number of columns based on available width
fn calculate_columns(width: u16, agent_count: usize) -> usize {
    if width < 60 {
        1
    } else if width < 100 {
        2.min(agent_count)
    } else if width < 140 {
        3.min(agent_count)
    } else {
        4.min(agent_count)
    }
}

/// Render a single agent card
fn render_agent_card(agent: &AgentInfo, area: Rect, frame: &mut Frame, is_selected: bool) {
    let border_style = if is_selected {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let status_color = match agent.status {
        AgentStatus::Running => Color::Green,
        AgentStatus::Completed => Color::Blue,
        AgentStatus::Error => Color::Red,
    };

    let status_text = match agent.status {
        AgentStatus::Running => "●",
        AgentStatus::Completed => "✓",
        AgentStatus::Error => "✗",
    };

    let title = format!("{} {}", status_text, agent.name);
    
    let mut lines = vec![
        format!("Task: {}", agent.task),
    ];

    if let Some(node) = &agent.current_node {
        lines.push(format!("Node: {}", node));
    }

    if let Some(msg) = &agent.progress_message {
        lines.push(format!("Progress: {}", msg));
    }

    let content = lines.join("\n");

    let card = Paragraph::new(content)
        .style(Style::default())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_style(Style::default().fg(status_color))
                .border_style(border_style),
        );

    frame.render_widget(card, area);
}
