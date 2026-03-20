//! UI components for the TUI application.
//!
//! This module contains all the UI components and rendering logic.

pub mod layout;
pub mod header;
pub mod agents;
pub mod session;
pub mod input;
pub mod message_block;

use ratatui::Frame;
use crate::tui::App;

/// Main render function that coordinates all UI components
pub fn render(frame: &mut Frame, app: &App) {
    let (header_area, main_area, input_area) = layout::LayoutCalculator::calculate_main_layout(frame.area());
    
    // Render header
    header::Header::render(frame, header_area, &app.state);
    
    // Render main area (agents and sessions)
    let (agent_area, session_area) = layout::LayoutCalculator::calculate_horizontal_layout(main_area);
    let agents: Vec<_> = app.state.agents.values().cloned().collect();
    agents::render_agents_grid(&agents, agent_area, frame, None);
    
    // Render session panel
    let mut panel = session::SessionPanel::new();
    panel.render(frame, session_area, &app.state.messages, &app.input_mode);
    
    // Render input bar
    input::render_input_bar(frame, input_area, &app.input, &app.input_mode, app.cursor_position);
}
