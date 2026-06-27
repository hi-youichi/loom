use std::io;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

use super::ui;

pub struct App {
    input: String,
    messages: Vec<String>,
    cursor_position: usize,
    status_text: String,
    should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            messages: Vec::new(),
            cursor_position: 0,
            status_text: "Ready".to_string(),
            should_quit: false,
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        crossterm::execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.run_loop(&mut terminal);

        disable_raw_mode()?;
        crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    fn run_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> io::Result<()> {
        loop {
            terminal.draw(|frame| ui::draw(frame, self))?;

            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key);
                    }
                }
            }

            if self.should_quit {
                return Ok(());
            }
        }
    }

    fn handle_key(&mut self, key: event::KeyEvent) {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Enter => {
                let line = self.input.clone();
                if !line.is_empty() {
                    self.messages.push(format!("> {}", line));
                    self.status_text = "Thinking...".to_string();
                    // TODO: send to agent, receive reply
                    self.messages.push("[agent reply placeholder]".to_string());
                    self.status_text = "Ready".to_string();
                }
                self.input.clear();
                self.cursor_position = 0;
            }
            KeyCode::Char(c) => {
                self.input.insert(self.cursor_position, c);
                self.cursor_position += c.len_utf8();
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    let prev = self.input[..self.cursor_position]
                        .chars()
                        .last()
                        .map(|c| c.len_utf8())
                        .unwrap_or(0);
                    self.cursor_position -= prev;
                    self.input.remove(self.cursor_position);
                }
            }
            KeyCode::Delete => {
                if self.cursor_position < self.input.len() {
                    self.input.remove(self.cursor_position);
                }
            }
            KeyCode::Left => {
                if self.cursor_position > 0 {
                    let prev = self.input[..self.cursor_position]
                        .chars()
                        .last()
                        .map(|c| c.len_utf8())
                        .unwrap_or(0);
                    self.cursor_position -= prev;
                }
            }
            KeyCode::Right => {
                if self.cursor_position < self.input.len() {
                    let next = self.input[self.cursor_position..]
                        .chars()
                        .next()
                        .map(|c| c.len_utf8())
                        .unwrap_or(0);
                    self.cursor_position += next;
                }
            }
            KeyCode::Home => {
                self.cursor_position = 0;
            }
            KeyCode::End => {
                self.cursor_position = self.input.len();
            }
            _ => {}
        }
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn cursor_position(&self) -> usize {
        self.cursor_position
    }

    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    pub fn status_text(&self) -> &str {
        &self.status_text
    }
}
