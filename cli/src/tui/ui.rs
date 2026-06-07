use ratatui::prelude::*;
use ratatui::widgets::*;

use super::app::App;

pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    draw_messages(frame, app, chunks[0]);
    draw_status_bar(frame, app, chunks[1]);
    draw_input(frame, app, chunks[2]);
}

fn draw_messages(frame: &mut Frame, app: &App, area: Rect) {
    let lines: Vec<Line> = app
        .messages()
        .iter()
        .map(|msg| Line::from(msg.as_str()))
        .collect();

    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((app.messages().len().saturating_sub(area.height as usize) as u16, 0));

    frame.render_widget(widget, area);
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let style = Style::default().fg(Color::Black).bg(Color::Cyan);
    let widget = Paragraph::new(Line::from(app.status_text())).style(style);
    frame.render_widget(widget, area);
}

fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let input_with_prompt = format!("> {}", app.input());
    let widget = Paragraph::new(Line::from(input_with_prompt.as_str()));
    frame.render_widget(widget, area);

    frame.set_cursor_position((
        area.x + 2 + app.cursor_position() as u16,
        area.y,
    ));
}
