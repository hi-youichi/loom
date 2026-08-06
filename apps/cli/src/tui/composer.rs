//! Input box component for the TUI — the Composer.
//!
//! Provides a multi-line text input with:
//!
//! - `Enter` to submit, `Shift+Enter` for newline
//! - History navigation (`↑` / `↓`)
//! - Cursor movement (`←` / `→` / `Home` / `End`)
//! - Word-skip with `Ctrl+←` / `Ctrl+→`
//! - `Ctrl+U` clear line, `Ctrl+W` delete word backward
//! - Slash-command detection (`/` prefix without space)
//! - Placeholder text when empty
//!
//! Renders as a bordered ratatui widget with a blinking-bar cursor.

use crossterm::cursor::SetCursorStyle;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::render::Renderable;

// ---------------------------------------------------------------------------
// ComposerAction
// ---------------------------------------------------------------------------

/// Result of processing a key event in the composer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerAction {
    /// Continue editing — the event was consumed but nothing was submitted.
    Continue,
    /// The user submitted the input text.
    Submit(String),
}

// ---------------------------------------------------------------------------
// Composer
// ---------------------------------------------------------------------------

/// Multi-line text input component.
///
/// The composer displays a bordered input area with placeholder text when
/// empty.  It supports:
///
/// - Multi-line input (`Shift+Enter` inserts a newline)
/// - History traversal (`↑` / `↓`)
/// - Cursor navigation (`←` / `→` / `Ctrl+←` / `Ctrl+→` / `Home` / `End`)
/// - Line editing (`Backspace` / `Delete` / `Ctrl+U` / `Ctrl+W`)
/// - Slash-command detection (starts with `/` and contains no space)
///
/// Typing is cumulative via `insert_text()`, which handles paste events
/// and autocomplete text.
pub struct Composer {
    /// Current input content.
    input: String,
    /// Cursor position as a **byte** index (always on a valid UTF-8 boundary).
    cursor: usize,
    /// Previously submitted strings (most recent at the end).
    history: Vec<String>,
    /// Current position in history; `None` means the user is typing new input.
    history_index: Option<usize>,
    /// Whether the current input starts with `/` (no spaces yet).
    slash_command: bool,
    /// Placeholder text shown when the input is empty.
    placeholder: String,
}

impl Composer {
    /// Create a new `Composer` with the default placeholder.
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            slash_command: false,
            placeholder: " 输入消息... (/help 查看命令)".to_string(),
        }
    }

    /// Create a `Composer` with a custom placeholder.
    pub fn with_placeholder(placeholder: impl Into<String>) -> Self {
        let mut s = Self::new();
        s.placeholder = placeholder.into();
        s
    }

    // ── Accessors ───────────────────────────────────────────────────────

    /// The current input content.
    pub fn content(&self) -> &str {
        &self.input
    }

    /// Whether the current input is a slash command (starts with `/`).
    pub fn is_slash_command(&self) -> bool {
        self.slash_command
    }

    /// The input history (previous submissions).
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// The current cursor position (byte index).
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    // ── Mutators ────────────────────────────────────────────────────────

    /// Submit the current input, clearing the buffer and adding it to history.
    ///
    /// Empty strings are **not** added to history.
    pub fn submit(&mut self) -> String {
        let content = std::mem::take(&mut self.input);
        if !content.is_empty() {
            self.history.push(content.clone());
        }
        self.cursor = 0;
        self.history_index = None;
        self.slash_command = false;
        content
    }

    /// Insert text at the cursor position (from typing, paste, or autocomplete).
    pub fn insert_text(&mut self, text: &str) {
        let before = &self.input[..self.cursor];
        let after = &self.input[self.cursor..];
        self.input = format!("{before}{text}{after}");
        self.cursor += text.len();
        self.update_slash_state();
    }

    /// Delete the character **before** the cursor (Backspace).
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.prev_char_boundary(self.cursor);
        let before = &self.input[..prev];
        let after = &self.input[self.cursor..];
        self.input = format!("{before}{after}");
        self.cursor = prev;
        self.update_slash_state();
    }

    /// Delete the character **at** the cursor (forward Delete).
    pub fn delete(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let next = self.next_char_boundary(self.cursor);
        let before = &self.input[..self.cursor];
        let after = &self.input[next..];
        self.input = format!("{before}{after}");
        self.update_slash_state();
    }

    /// Delete the word before the cursor (`Ctrl+W`).
    pub fn delete_word_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let before = &self.input[..self.cursor];
        let trimmed = before.trim_end();
        let word_start = trimmed[..trimmed.len()]
            .rfind(|c: char| c == ' ')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        let after = &self.input[self.cursor..];
        self.input = format!("{}{}", &before[..word_start], after);
        self.cursor = word_start;
        self.update_slash_state();
    }

    /// Clear the entire input buffer.
    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.slash_command = false;
    }

    // ── Key event handling ──────────────────────────────────────────────

    /// Process a key event and return the resulting action.
    ///
    /// This is the main entry point for keyboard input.  Most events are
    /// consumed internally and produce [`ComposerAction::Continue`].
    /// Only `Enter` (without `Shift`) can produce [`ComposerAction::Submit`].
    pub fn handle_key(&mut self, key: KeyEvent) -> ComposerAction {
        match key.code {
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift+Enter → insert a literal newline
                    self.insert_text("\n");
                    ComposerAction::Continue
                } else {
                    // Enter → submit (skip purely whitespace input)
                    if self.input.trim().is_empty() {
                        ComposerAction::Continue
                    } else {
                        let content = self.submit();
                        ComposerAction::Submit(content)
                    }
                }
            }

            KeyCode::Up => {
                self.navigate_history(-1);
                ComposerAction::Continue
            }
            KeyCode::Down => {
                self.navigate_history(1);
                ComposerAction::Continue
            }

            KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_word_left();
                ComposerAction::Continue
            }
            KeyCode::Left => {
                self.move_left();
                ComposerAction::Continue
            }
            KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_word_right();
                ComposerAction::Continue
            }
            KeyCode::Right => {
                self.move_right();
                ComposerAction::Continue
            }

            KeyCode::Home => {
                self.cursor = 0;
                ComposerAction::Continue
            }
            KeyCode::End => {
                self.cursor = self.input.len();
                ComposerAction::Continue
            }

            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => match c {
                'u' => {
                    // Ctrl+U: clear entire line
                    self.clear();
                    ComposerAction::Continue
                }
                'w' => {
                    // Ctrl+W: delete word backward
                    self.delete_word_backward();
                    ComposerAction::Continue
                }
                _ => {
                    // Other Ctrl+char — ignored here (App handles Ctrl+C/D)
                    ComposerAction::Continue
                }
            },
            KeyCode::Char(c) => {
                self.insert_text(&c.to_string());
                ComposerAction::Continue
            }

            KeyCode::Backspace => {
                self.backspace();
                ComposerAction::Continue
            }
            KeyCode::Delete => {
                self.delete();
                ComposerAction::Continue
            }

            KeyCode::Tab => {
                // Tab: autocomplete placeholder (to be implemented)
                ComposerAction::Continue
            }

            _ => ComposerAction::Continue,
        }
    }

    // ── Cursor movement (private) ───────────────────────────────────────

    fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.prev_char_boundary(self.cursor);
        }
    }

    fn move_right(&mut self) {
        if self.cursor < self.input.len() {
            self.cursor = self.next_char_boundary(self.cursor);
        }
    }

    /// Jump to the start of the previous word (or the start of the line).
    fn move_word_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let before = &self.input[..self.cursor];
        // Skip trailing spaces, then find the last space before the word.
        let trimmed = before.trim_end();
        let word_start = trimmed[..trimmed.len()]
            .rfind(|c: char| c == ' ')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        self.cursor = word_start;
    }

    /// Jump to the start of the next word (or the end of the line).
    fn move_word_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let after = &self.input[self.cursor..];
        // Skip the current word
        let word_end = after.find(|c: char| c == ' ').unwrap_or(after.len());
        // Skip the spaces after the word
        let after_word = &after[word_end..];
        let space_end = after_word
            .find(|c: char| c != ' ')
            .unwrap_or(after_word.len());
        self.cursor += word_end + space_end;
    }

    // ── History navigation (private) ────────────────────────────────────

    /// Navigate history: `-1` = older (up), `+1` = newer (down).
    ///
    /// When the user presses `↑` from a fresh input, the current draft is
    /// replaced by the most recent history entry.  Pressing `↓` at the
    /// newest entry does nothing.
    fn navigate_history(&mut self, direction: i32) {
        if self.history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                // Currently on a new draft — save nothing, just jump.
                if direction < 0 {
                    self.history_index = Some(self.history.len() - 1);
                    self.input = self.history[self.history.len() - 1].clone();
                    self.cursor = self.input.len();
                }
            }
            Some(idx) => {
                let new_idx = if direction < 0 {
                    if idx > 0 {
                        idx - 1
                    } else {
                        return; // Already at the oldest.
                    }
                } else {
                    if idx + 1 < self.history.len() {
                        idx + 1
                    } else {
                        return; // Already at the newest.
                    }
                };
                self.history_index = Some(new_idx);
                self.input = self.history[new_idx].clone();
                self.cursor = self.input.len();
            }
        }
    }

    // ── Slash-command state (private) ───────────────────────────────────

    /// Update the `slash_command` flag based on current input.
    fn update_slash_state(&mut self) {
        self.slash_command = self.input.starts_with('/') && !self.input.contains(' ');
    }

    // ── UTF-8 boundary helpers (private) ─────────────────────────────────

    /// Find the byte index of the previous character boundary.
    fn prev_char_boundary(&self, byte_pos: usize) -> usize {
        self.input[..byte_pos]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    /// Find the byte index of the next character boundary.
    fn next_char_boundary(&self, byte_pos: usize) -> usize {
        self.input[byte_pos..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| byte_pos + i)
            .unwrap_or(self.input.len())
    }
}

impl Default for Composer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Renderable implementation
// ---------------------------------------------------------------------------

impl Renderable for Composer {
    /// Render the composer as a bordered input box with placeholder or content.
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(" 输入 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let display = if self.input.is_empty() {
            // Show placeholder in dim style
            Span::styled(
                self.placeholder.clone(),
                Style::default().fg(Color::DarkGray),
            )
        } else {
            Span::raw(self.input.clone())
        };

        let paragraph = Paragraph::new(Line::from(display))
            .block(block)
            .wrap(Wrap { trim: false });

        paragraph.render(area, buf);
    }

    /// Report the desired height based on the number of input lines.
    ///
    /// Minimum 3 lines: 1 for input + 2 for borders.  Grows with multi-line
    /// content.  Line wrapping is handled by the ratatui `Paragraph` widget,
    /// so this reports the *logical* line count.
    fn desired_height(&self, _width: u16) -> u16 {
        let line_count = if self.input.is_empty() {
            1
        } else {
            self.input.lines().count().max(1) as u16
        };
        // +2 for top and bottom borders
        line_count + 2
    }

    /// Compute the visual cursor position inside the bordered area.
    ///
    /// Returns `(x, y)` relative to the full `area` (including borders),
    /// offset by 1 in both directions to account for the border.
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let text_before = &self.input[..self.cursor];
        let lines: Vec<&str> = text_before.split('\n').collect();
        let row = lines.len().saturating_sub(1) as u16;
        let col = UnicodeWidthStr::width(lines.last().copied().unwrap_or("")) as u16;

        // +1 for the left border
        let x = area.x + 1 + col;
        let y = area.y + 1 + row;

        // Clamp to the widget area (minus borders)
        let max_x = area.x + area.width.saturating_sub(1);
        let max_y = area.y + area.height.saturating_sub(1);
        Some((x.min(max_x), y.min(max_y)))
    }

    /// Use a blinking bar cursor for the input field.
    fn cursor_style(&self, _area: Rect) -> SetCursorStyle {
        SetCursorStyle::BlinkingBar
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};
    use ratatui::buffer::Buffer as TuiBuffer;
    use ratatui::layout::Rect;

    // ── Helpers ─────────────────────────────────────────────────────────

    /// Build a press key event with no modifiers.
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// Build a press key event with a modifier.
    fn key_with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    // ── Construction ────────────────────────────────────────────────────

    #[test]
    fn test_new_composer_is_empty() {
        let c = Composer::new();
        assert_eq!(c.content(), "");
        assert_eq!(c.cursor(), 0);
        assert!(!c.is_slash_command());
        assert!(c.history().is_empty());
    }

    #[test]
    fn test_default_equals_new() {
        assert_eq!(Composer::default().content(), Composer::new().content());
    }

    #[test]
    fn test_with_placeholder() {
        let c = Composer::with_placeholder("测试");
        // placeholder is only visual, not exposed via content()
        assert_eq!(c.content(), "");
    }

    // ── Text insertion ──────────────────────────────────────────────────

    #[test]
    fn test_insert_text_appends_at_cursor() {
        let mut c = Composer::new();
        c.insert_text("hello");
        assert_eq!(c.content(), "hello");
        assert_eq!(c.cursor(), 5);
    }

    #[test]
    fn test_insert_text_mid_string() {
        let mut c = Composer::new();
        c.insert_text("helo");
        c.cursor = 3;
        c.insert_text("l");
        assert_eq!(c.content(), "hello");
        assert_eq!(c.cursor(), 4);
    }

    #[test]
    fn test_insert_text_unicode() {
        let mut c = Composer::new();
        c.insert_text("héllo");
        assert_eq!(c.content(), "héllo");
        // 'é' is 2 bytes in UTF-8
        assert_eq!(c.cursor(), 6);
    }

    // ── Backspace ───────────────────────────────────────────────────────

    #[test]
    fn test_backspace_empty() {
        let mut c = Composer::new();
        c.backspace(); // no crash
        assert_eq!(c.content(), "");
    }

    #[test]
    fn test_backspace_removes_before_cursor() {
        let mut c = Composer::new();
        c.insert_text("hello");
        c.backspace();
        assert_eq!(c.content(), "hell");
        assert_eq!(c.cursor(), 4);
    }

    #[test]
    fn test_backspace_mid_string() {
        let mut c = Composer::new();
        c.insert_text("hllo");
        c.cursor = 1;
        c.insert_text("e");
        // "hello" cursor=2
        c.cursor = 2;
        c.backspace();
        assert_eq!(c.content(), "hllo");
        assert_eq!(c.cursor(), 1);
    }

    #[test]
    fn test_backspace_unicode() {
        let mut c = Composer::new();
        c.insert_text("héllo");
        c.backspace(); // removes 'o'
        assert_eq!(c.content(), "héll");
        c.backspace(); // removes 'l'
        assert_eq!(c.content(), "hél");
        // Move back through 'é' (2 bytes)
        c.backspace(); // removes 'l'
        assert_eq!(c.content(), "hé");
        c.backspace(); // removes 'é' (2 bytes)
        assert_eq!(c.content(), "h");
        c.backspace();
        assert_eq!(c.content(), "");
    }

    // ── Delete (forward) ────────────────────────────────────────────────

    #[test]
    fn test_delete_at_end_does_nothing() {
        let mut c = Composer::new();
        c.insert_text("hello");
        c.delete(); // cursor at end, no-op
        assert_eq!(c.content(), "hello");
    }

    #[test]
    fn test_delete_mid_string() {
        let mut c = Composer::new();
        c.insert_text("hllo");
        c.cursor = 1;
        c.delete(); // removes 'l' at cursor
        assert_eq!(c.content(), "hlo");
        assert_eq!(c.cursor(), 1);
    }

    // ── Submit ──────────────────────────────────────────────────────────

    #[test]
    fn test_submit_returns_content_and_clears() {
        let mut c = Composer::new();
        c.insert_text("hello");
        let result = c.submit();
        assert_eq!(result, "hello");
        assert_eq!(c.content(), "");
        assert_eq!(c.history().len(), 1);
        assert_eq!(c.history()[0], "hello");
    }

    #[test]
    fn test_submit_empty_does_not_add_to_history() {
        let mut c = Composer::new();
        let result = c.submit();
        assert_eq!(result, "");
        assert!(c.history().is_empty());
    }

    // ── Key handling ────────────────────────────────────────────────────

    #[test]
    fn test_handle_key_enter_submits_non_empty() {
        let mut c = Composer::new();
        c.insert_text("test");
        let action = c.handle_key(key(KeyCode::Enter));
        assert_eq!(action, ComposerAction::Submit("test".into()));
        assert!(c.content().is_empty());
    }

    #[test]
    fn test_handle_key_enter_skips_empty() {
        let mut c = Composer::new();
        let action = c.handle_key(key(KeyCode::Enter));
        assert_eq!(action, ComposerAction::Continue);
    }

    #[test]
    fn test_handle_key_shift_enter_inserts_newline() {
        let mut c = Composer::new();
        c.insert_text("a");
        let action = c.handle_key(key_with(KeyCode::Enter, KeyModifiers::SHIFT));
        assert_eq!(action, ComposerAction::Continue);
        assert_eq!(c.content(), "a\n");
    }

    #[test]
    fn test_handle_key_char_inserts() {
        let mut c = Composer::new();
        let action = c.handle_key(key(KeyCode::Char('x')));
        assert_eq!(action, ComposerAction::Continue);
        assert_eq!(c.content(), "x");
    }

    #[test]
    fn test_handle_key_backspace() {
        let mut c = Composer::new();
        c.insert_text("ab");
        let action = c.handle_key(key(KeyCode::Backspace));
        assert_eq!(action, ComposerAction::Continue);
        assert_eq!(c.content(), "a");
    }

    #[test]
    fn test_handle_key_left_right() {
        let mut c = Composer::new();
        c.insert_text("ab");
        c.handle_key(key(KeyCode::Left));
        assert_eq!(c.cursor(), 1);
        c.handle_key(key(KeyCode::Right));
        assert_eq!(c.cursor(), 2);
        c.handle_key(key(KeyCode::Left));
        c.handle_key(key(KeyCode::Left));
        assert_eq!(c.cursor(), 0);
        // Left at start does nothing
        c.handle_key(key(KeyCode::Left));
        assert_eq!(c.cursor(), 0);
    }

    #[test]
    fn test_handle_key_home_end() {
        let mut c = Composer::new();
        c.insert_text("hello");
        c.cursor = 3;
        c.handle_key(key(KeyCode::Home));
        assert_eq!(c.cursor(), 0);
        c.handle_key(key(KeyCode::End));
        assert_eq!(c.cursor(), 5);
    }

    #[test]
    fn test_handle_key_delete() {
        let mut c = Composer::new();
        c.insert_text("abc");
        c.cursor = 1;
        c.handle_key(key(KeyCode::Delete));
        assert_eq!(c.content(), "ac");
        assert_eq!(c.cursor(), 1);
    }

    #[test]
    fn test_handle_key_tab_is_noop() {
        let mut c = Composer::new();
        let action = c.handle_key(key(KeyCode::Tab));
        assert_eq!(action, ComposerAction::Continue);
    }

    #[test]
    fn test_handle_ctrl_u_clears() {
        let mut c = Composer::new();
        c.insert_text("hello world");
        let action = c.handle_key(key_with(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(action, ComposerAction::Continue);
        assert!(c.content().is_empty());
    }

    #[test]
    fn test_handle_ctrl_w_deletes_word() {
        let mut c = Composer::new();
        c.insert_text("hello world");
        c.cursor = 11; // end
        let action = c.handle_key(key_with(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(action, ComposerAction::Continue);
        assert_eq!(c.content(), "hello ");
    }

    // ── History navigation ──────────────────────────────────────────────

    #[test]
    fn test_history_up_down() {
        let mut c = Composer::new();
        c.insert_text("first");
        c.submit();
        c.insert_text("second");
        c.submit();

        // Up from new input → most recent
        c.handle_key(key(KeyCode::Up));
        assert_eq!(c.content(), "second");
        assert_eq!(c.history_index, Some(1));

        // Up again → older
        c.handle_key(key(KeyCode::Up));
        assert_eq!(c.content(), "first");
        assert_eq!(c.history_index, Some(0));

        // Up at oldest → stays
        c.handle_key(key(KeyCode::Up));
        assert_eq!(c.content(), "first");

        // Down → newer
        c.handle_key(key(KeyCode::Down));
        assert_eq!(c.content(), "second");
        assert_eq!(c.history_index, Some(1));

        // Down at newest → stays
        c.handle_key(key(KeyCode::Down));
        assert_eq!(c.content(), "second");
    }

    #[test]
    fn test_history_empty_up_down_does_nothing() {
        let mut c = Composer::new();
        c.handle_key(key(KeyCode::Up));
        assert!(c.content().is_empty());
        c.handle_key(key(KeyCode::Down));
        assert!(c.content().is_empty());
    }

    // ── Slash command detection ─────────────────────────────────────────

    #[test]
    fn test_slash_command_detected() {
        let mut c = Composer::new();
        c.insert_text("/help");
        assert!(c.is_slash_command());
    }

    #[test]
    fn test_slash_command_with_space() {
        let mut c = Composer::new();
        c.insert_text("/help me");
        assert!(!c.is_slash_command());
    }

    #[test]
    fn test_slash_command_clears_on_backspace() {
        let mut c = Composer::new();
        c.insert_text("/");
        assert!(c.is_slash_command());
        c.backspace();
        assert!(!c.is_slash_command());
    }

    // ── Renderable traits ───────────────────────────────────────────────

    #[test]
    fn test_desired_height_empty() {
        let c = Composer::new();
        assert_eq!(c.desired_height(80), 3); // 1 + 2 borders
    }

    #[test]
    fn test_desired_height_single_line() {
        let mut c = Composer::new();
        c.insert_text("hello");
        assert_eq!(c.desired_height(80), 3); // 1 + 2 borders
    }

    #[test]
    fn test_desired_height_multi_line() {
        let mut c = Composer::new();
        c.insert_text("hello\nworld");
        assert_eq!(c.desired_height(80), 4); // 2 lines + 2 borders
    }

    #[test]
    fn test_cursor_pos_empty() {
        let c = Composer::new();
        let pos = c.cursor_pos(Rect::new(0, 0, 80, 3));
        // (x=0+1, y=0+1) = (1, 1)
        assert_eq!(pos, Some((1, 1)));
    }

    #[test]
    fn test_cursor_pos_after_text() {
        let mut c = Composer::new();
        c.insert_text("hi");
        let pos = c.cursor_pos(Rect::new(0, 0, 80, 3));
        // col = 2, row = 0 → (0+1+2, 0+1+0) = (3, 1)
        assert_eq!(pos, Some((3, 1)));
    }

    #[test]
    fn test_cursor_pos_multi_line() {
        let mut c = Composer::new();
        c.insert_text("a\nbc");
        // cursor at end = byte 5
        let pos = c.cursor_pos(Rect::new(0, 0, 80, 5));
        // col = width("bc") = 2, row = 1 → (0+1+2, 0+1+1) = (3, 2)
        assert_eq!(pos, Some((3, 2)));
    }

    #[test]
    fn test_cursor_style_blinking_bar() {
        let c = Composer::new();
        assert_eq!(c.cursor_style(Rect::new(0, 0, 80, 3)), SetCursorStyle::BlinkingBar);
    }

    // ── Word navigation ─────────────────────────────────────────────────

    #[test]
    fn test_word_left() {
        let mut c = Composer::new();
        c.insert_text("hello world foo");
        c.cursor = 15; // end
        c.handle_key(key_with(KeyCode::Left, KeyModifiers::CONTROL));
        assert_eq!(c.cursor(), 12); // " foo" → start of "foo"
    }

    #[test]
    fn test_word_right() {
        let mut c = Composer::new();
        c.insert_text("hello world foo");
        c.cursor = 0;
        c.handle_key(key_with(KeyCode::Right, KeyModifiers::CONTROL));
        assert_eq!(c.cursor(), 6); // "hello " → start of "world"
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn test_clear() {
        let mut c = Composer::new();
        c.insert_text("hello");
        c.clear();
        assert_eq!(c.content(), "");
        assert_eq!(c.cursor(), 0);
        assert!(!c.is_slash_command());
    }

    #[test]
    fn test_delete_word_backward_empty() {
        let mut c = Composer::new();
        c.delete_word_backward(); // no crash
    }

    #[test]
    fn test_delete_word_backward_single_word() {
        let mut c = Composer::new();
        c.insert_text("hello");
        c.cursor = 5;
        c.delete_word_backward();
        assert_eq!(c.content(), "");
    }

    #[test]
    fn test_render_no_panic_on_zero_area() {
        let c = Composer::new();
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = TuiBuffer::empty(area);
        c.render(area, &mut buf); // no crash
    }

    // ── Additional boundary & edge case tests ──────────────────────────

    #[test]
    fn test_handle_key_enter_whitespace_only() {
        let mut c = Composer::new();
        c.insert_text("   ");
        let action = c.handle_key(key(KeyCode::Enter));
        assert_eq!(action, ComposerAction::Continue);
        // Content should NOT be cleared — whitespace-only input is not submitted
        assert_eq!(c.content(), "   ");
    }

    #[test]
    fn test_insert_text_empty() {
        let mut c = Composer::new();
        c.insert_text("hello");
        let cursor = c.cursor();
        c.insert_text(""); // no-op
        assert_eq!(c.content(), "hello");
        assert_eq!(c.cursor(), cursor);
    }

    #[test]
    fn test_cursor_pos_zero_area() {
        let c = Composer::new();
        let pos = c.cursor_pos(Rect::new(0, 0, 0, 0));
        // x = 0+1+0 = 1, clamped to max_x = 0+0-1 = 0
        // y = 0+1+0 = 1, clamped to max_y = 0+0-1 = 0
        assert_eq!(pos, Some((0, 0)));
    }

    #[test]
    fn test_cursor_pos_clamped_to_area() {
        let mut c = Composer::new();
        c.insert_text("hello world this is a very long line of text");
        let pos = c.cursor_pos(Rect::new(0, 0, 5, 3));
        // x = 0+1+width("...") = 1+38 = 39, clamped to max_x = 0+5-1 = 4
        // y = 0+1+0 = 1, clamped to max_y = 0+3-1 = 2
        assert_eq!(pos, Some((4, 1)));
    }

    #[test]
    fn test_cursor_pos_unicode_multi_byte() {
        let mut c = Composer::new();
        c.insert_text("aébc");
        // cursor at end = byte 6 (é is 2 bytes)
        let pos = c.cursor_pos(Rect::new(0, 0, 80, 3));
        // col = UnicodeWidthStr::width("aébc") = 4
        // x = 0+1+4 = 5
        assert_eq!(pos, Some((5, 1)));
    }

    #[test]
    fn test_cursor_pos_mid_string_unicode() {
        let mut c = Composer::new();
        c.insert_text("aébc");
        c.cursor = 1; // cursor after 'a', before 'é' (byte 1)
        let pos = c.cursor_pos(Rect::new(0, 0, 80, 3));
        // col = UnicodeWidthStr::width("a") = 1
        assert_eq!(pos, Some((2, 1)));
    }

    #[test]
    fn test_desired_height_empty_lines() {
        let mut c = Composer::new();
        c.insert_text("\n\n"); // 2 lines (Rust's .lines() skips trailing empty from final \n)
        assert_eq!(c.desired_height(80), 4); // 2 lines + 2 borders
    }

    #[test]
    fn test_desired_height_zero_width() {
        let mut c = Composer::new();
        c.insert_text("hello\nworld");
        // desired_height does not depend on width for logical line count
        assert_eq!(c.desired_height(0), 4);
        let c_empty = Composer::new();
        assert_eq!(c_empty.desired_height(0), 3);
    }

    // ── State transitions ──────────────────────────────────────────────

    #[test]
    fn test_slash_command_after_submit() {
        let mut c = Composer::new();
        c.insert_text("/help");
        assert!(c.is_slash_command());
        c.submit();
        assert!(!c.is_slash_command());
    }

    #[test]
    fn test_slash_command_after_clear() {
        let mut c = Composer::new();
        c.insert_text("/help");
        assert!(c.is_slash_command());
        c.clear();
        assert!(!c.is_slash_command());
    }

    #[test]
    fn test_slash_command_via_delete() {
        let mut c = Composer::new();
        c.insert_text("/help");
        c.cursor = 5; // end
        c.delete(); // no-op at end
        assert!(c.is_slash_command());
    }

    #[test]
    fn test_slash_command_clears_when_space_inserted() {
        let mut c = Composer::new();
        c.insert_text("/he");
        assert!(c.is_slash_command());
        c.insert_text("llo world");
        assert!(!c.is_slash_command());
    }

    #[test]
    fn test_history_index_after_submit() {
        let mut c = Composer::new();
        c.insert_text("hello");
        c.submit();
        // After submit, history_index should be None (observable via Up behavior)
        c.handle_key(key(KeyCode::Up));
        assert_eq!(c.content(), "hello");
    }

    #[test]
    fn test_cursor_after_submit_is_zero() {
        let mut c = Composer::new();
        c.insert_text("hello");
        c.cursor = 3;
        c.submit();
        assert_eq!(c.cursor(), 0);
    }

    #[test]
    fn test_cursor_after_clear_is_zero() {
        let mut c = Composer::new();
        c.insert_text("hello");
        c.cursor = 3;
        c.clear();
        assert_eq!(c.cursor(), 0);
    }

    // ── Event handling edge cases ──────────────────────────────────────

    #[test]
    fn test_handle_key_unknown_code() {
        let mut c = Composer::new();
        let action = c.handle_key(key(KeyCode::F(1)));
        assert_eq!(action, ComposerAction::Continue);
        assert!(c.content().is_empty());
    }

    #[test]
    fn test_handle_key_ctrl_other() {
        let mut c = Composer::new();
        // Ctrl+A, Ctrl+B, etc. (not u or w) should be ignored
        let action = c.handle_key(key_with(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(action, ComposerAction::Continue);
        assert!(c.content().is_empty());
    }

    #[test]
    fn test_handle_key_multiple_chars() {
        let mut c = Composer::new();
        for ch in "hello".chars() {
            c.handle_key(key(KeyCode::Char(ch)));
        }
        assert_eq!(c.content(), "hello");
        assert_eq!(c.cursor(), 5);
    }

    // ── Word navigation additional edge cases ──────────────────────────

    #[test]
    fn test_move_word_left_at_start() {
        let mut c = Composer::new();
        c.insert_text("hello world");
        c.cursor = 0;
        c.handle_key(key_with(KeyCode::Left, KeyModifiers::CONTROL));
        assert_eq!(c.cursor(), 0);
    }

    #[test]
    fn test_move_word_right_at_end() {
        let mut c = Composer::new();
        c.insert_text("hello world");
        c.cursor = 11;
        c.handle_key(key_with(KeyCode::Right, KeyModifiers::CONTROL));
        assert_eq!(c.cursor(), 11);
    }

    #[test]
    fn test_move_word_left_from_spaces() {
        let mut c = Composer::new();
        c.insert_text("hello   world");
        c.cursor = 10; // in the middle of spaces
        c.handle_key(key_with(KeyCode::Left, KeyModifiers::CONTROL));
        // Jumps to start of "world" (index 8) — the last space before cursor
        assert_eq!(c.cursor(), 8);
    }

    #[test]
    fn test_move_word_right_from_spaces() {
        let mut c = Composer::new();
        c.insert_text("hello   world");
        c.cursor = 5; // at the start of spaces
        c.handle_key(key_with(KeyCode::Right, KeyModifiers::CONTROL));
        // Should jump to "world" (index 8)
        assert_eq!(c.cursor(), 8);
    }

    #[test]
    fn test_move_word_left_single_word() {
        let mut c = Composer::new();
        c.insert_text("hello");
        c.cursor = 5;
        c.handle_key(key_with(KeyCode::Left, KeyModifiers::CONTROL));
        // Only one word, should jump to start
        assert_eq!(c.cursor(), 0);
    }

    // ── Delete word backward additional edge cases ─────────────────────

    #[test]
    fn test_delete_word_backward_multiple_words() {
        let mut c = Composer::new();
        c.insert_text("hello world foo");
        c.cursor = 15; // end
        c.delete_word_backward();
        // Removes " foo" → "hello world " (trailing space preserved)
        assert_eq!(c.content(), "hello world ");
        assert_eq!(c.cursor(), 12);
    }

    #[test]
    fn test_delete_word_backward_with_trailing_spaces() {
        let mut c = Composer::new();
        c.insert_text("hello world   ");
        c.cursor = 14; // after trailing spaces (5+1+5+3 = 14)
        c.delete_word_backward();
        // trim_end removes trailing spaces, finds "world" → "hello "
        assert_eq!(c.content(), "hello ");
        assert_eq!(c.cursor(), 6);
    }

    #[test]
    fn test_delete_word_backward_cursor_mid_word() {
        let mut c = Composer::new();
        c.insert_text("hello world");
        c.cursor = 8; // at 'o' in 'world'
        c.delete_word_backward();
        // before = "hello wo", trim_end = "hello wo", rfind(' ') = 5
        // word_start = 6, after = "rld" → "hello rld"
        assert_eq!(c.content(), "hello rld");
        assert_eq!(c.cursor(), 6);
    }

    #[test]
    fn test_delete_word_backward_cursor_at_word_boundary() {
        let mut c = Composer::new();
        c.insert_text("hello world");
        c.cursor = 6; // at the space
        c.delete_word_backward();
        // before = "hello ", trim_end = "hello", rfind(' ') = None
        // word_start = 0, after = "world" → "world"
        assert_eq!(c.content(), "world");
        assert_eq!(c.cursor(), 0);
    }

    // ── History additional edge cases ──────────────────────────────────

    #[test]
    fn test_history_down_from_new_input_does_nothing() {
        let mut c = Composer::new();
        c.insert_text("first");
        c.submit();
        // Currently on new input (history_index = None)
        c.handle_key(key(KeyCode::Down));
        // Down from new input should do nothing (only up triggers history)
        assert!(c.content().is_empty());
    }

    #[test]
    fn test_history_single_entry_up_down() {
        let mut c = Composer::new();
        c.insert_text("only");
        c.submit();

        // Up → retrieve "only"
        c.handle_key(key(KeyCode::Up));
        assert_eq!(c.content(), "only");

        // Down → stays (it's the only entry, so down at newest stays)
        c.handle_key(key(KeyCode::Down));
        assert_eq!(c.content(), "only");
    }

    #[test]
    fn test_history_up_then_down_then_up_cycle() {
        let mut c = Composer::new();
        c.insert_text("first");
        c.submit();
        c.insert_text("second");
        c.submit();
        c.insert_text("third");
        c.submit();

        // Up → "third"
        c.handle_key(key(KeyCode::Up));
        assert_eq!(c.content(), "third");
        // Up → "second"
        c.handle_key(key(KeyCode::Up));
        assert_eq!(c.content(), "second");
        // Down → "third"
        c.handle_key(key(KeyCode::Down));
        assert_eq!(c.content(), "third");
        // Down → stays (newest)
        c.handle_key(key(KeyCode::Down));
        assert_eq!(c.content(), "third");
    }

    // ── Renderable trait — render with content ─────────────────────────

    #[test]
    fn test_render_with_content_no_panic() {
        let mut c = Composer::new();
        c.insert_text("hello\nworld");
        let area = Rect::new(0, 0, 40, 5);
        let mut buf = TuiBuffer::empty(area);
        c.render(area, &mut buf); // no crash
    }

    #[test]
    fn test_render_with_placeholder_no_panic() {
        let c = Composer::new();
        let area = Rect::new(0, 0, 40, 3);
        let mut buf = TuiBuffer::empty(area);
        c.render(area, &mut buf); // no crash — should render placeholder
    }

    #[test]
    fn test_render_large_content_no_panic() {
        let mut c = Composer::new();
        let long_text = "hello world this is a very long line ".repeat(10);
        c.insert_text(&long_text);
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = TuiBuffer::empty(area);
        c.render(area, &mut buf); // no crash
    }

    // ── Renderable trait — cursor_pos advanced ─────────────────────────

    #[test]
    fn test_cursor_pos_multi_line_unicode() {
        let mut c = Composer::new();
        c.insert_text("中文\n测试");
        // cursor at end = byte 13 (Chinese chars are 3 bytes each)
        // text_before = "中文\n测试"
        // lines = ["中文", "测试"]
        // row = 1, col = UnicodeWidthStr::width("测试") = 4
        let pos = c.cursor_pos(Rect::new(0, 0, 80, 5));
        // x = 0+1+4 = 5, y = 0+1+1 = 2
        assert_eq!(pos, Some((5, 2)));
    }

    #[test]
    fn test_cursor_pos_first_line_of_multi_line() {
        let mut c = Composer::new();
        c.insert_text("ab\ncd");
        c.cursor = 1; // after 'a' on first line
        let pos = c.cursor_pos(Rect::new(0, 0, 80, 5));
        // text_before = "a"
        // lines = ["a"]
        // row = 0, col = 1
        // x = 0+1+1 = 2, y = 0+1+0 = 1
        assert_eq!(pos, Some((2, 1)));
    }

    // ── ComposerAction enum ────────────────────────────────────────────

    #[test]
    fn test_composer_action_debug_and_partial_eq() {
        assert_eq!(ComposerAction::Continue, ComposerAction::Continue);
        assert_ne!(ComposerAction::Continue, ComposerAction::Submit("x".into()));
        assert_eq!(
            ComposerAction::Submit("hello".into()),
            ComposerAction::Submit("hello".into())
        );
        assert_ne!(
            ComposerAction::Submit("hello".into()),
            ComposerAction::Submit("world".into())
        );

        // Debug formatting
        let debug = format!("{:?}", ComposerAction::Continue);
        assert_eq!(debug, "Continue");
        let debug = format!("{:?}", ComposerAction::Submit("test".into()));
        assert_eq!(debug, "Submit(\"test\")");
    }
}