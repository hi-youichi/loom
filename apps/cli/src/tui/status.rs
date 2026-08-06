//! AI status indicator — status bar showing the current AI state.
//!
//! Renders a single-line status bar at the bottom of the TUI, showing the
//! current AI operational state (Idle, Thinking, Executing, WaitingApproval,
//! Error) with optional spinner animation and context message.
//!
//! The spinner reuses the 10-frame braille-dot animation concept from
//! [`display/spinner.rs`](crate::display::spinner), but as a lightweight
//! in-process frame counter rather than a background thread.
//!
//! # Integration
//!
//! ```ignore
//! let mut status_bar = StatusBar::new();
//! status_bar.set_status(AiStatus::Thinking);
//! status_bar.set_message(Some("Generating response...".into()));
//! ```
//!
//! Call `tick()` on each animation frame (e.g. every 150ms) to advance
//! the spinner while in `Thinking` or `Executing` states.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::tui::render::Renderable;

// ---------------------------------------------------------------------------
// AiStatus
// ---------------------------------------------------------------------------

/// AI operational status displayed in the status bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AiStatus {
    /// Idle, waiting for user input.
    Idle,
    /// Submitting input to the AI agent.
    Submitting,
    /// AI is thinking / generating a response.
    Thinking,
    /// AI is executing a tool call.
    Executing,
    /// Waiting for user approval (e.g. tool confirmation).
    WaitingApproval,
    /// Interrupted (e.g. Ctrl+C).
    Interrupted,
    /// An error occurred during execution.
    Error,
}

// ---------------------------------------------------------------------------
// Spinner frames
// ---------------------------------------------------------------------------

/// Spinner animation frames (Braille dots, 10 frames, ~150ms tick interval).
///
/// Reuses the same frame set from [`display::spinner::SPINNER_FRAMES`].
const SPINNER_FRAMES: &[char] = &[
    '⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏',
];

// ---------------------------------------------------------------------------
// StatusBar
// ---------------------------------------------------------------------------

/// Status bar widget indicating the current AI state.
///
/// Renders a single row with:
/// - A status label (e.g. ` 思考中 ⠋ `)
/// - An optional context message (e.g. tool name, error detail)
/// - A top border line separating it from the content area above
///
/// Call [`tick()`](Self::tick) on each animation frame to advance the
/// spinner while in `Thinking` or `Executing` states.
pub struct StatusBar {
    /// Current AI status.
    status: AiStatus,
    /// Current spinner frame index (`0 .. SPINNER_FRAMES.len()`).
    spinner_frame: usize,
    /// Optional context message (tool name, action description, error detail).
    message: Option<String>,
}

impl StatusBar {
    /// Create a new status bar in `Idle` state with no message.
    pub fn new() -> Self {
        Self {
            status: AiStatus::Idle,
            spinner_frame: 0,
            message: None,
        }
    }

    /// Update the current AI status.
    pub fn set_status(&mut self, status: AiStatus) {
        self.status = status;
    }

    /// Set or clear the optional context message.
    ///
    /// The message appears alongside the status label for `Thinking` and
    /// `Executing` states, and as the sole text for `Error` state.
    pub fn set_message(&mut self, message: Option<String>) {
        self.message = message;
    }

    /// Advance the spinner animation by one frame.
    ///
    /// Call this on each animation tick (e.g. every 150ms) to animate
    /// the spinner while in `Thinking` or `Executing` states. The frame
    /// index wraps around at `SPINNER_FRAMES.len()`.
    pub fn tick(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
    }

    /// Return the current status.
    pub fn status(&self) -> AiStatus {
        self.status
    }

    /// Return a reference to the current context message.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Reset to idle state with no message.
    pub fn reset(&mut self) {
        self.status = AiStatus::Idle;
        self.spinner_frame = 0;
        self.message = None;
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Renderable impl
// ---------------------------------------------------------------------------

impl Renderable for StatusBar {
    /// Render the status bar into the given buffer area.
    ///
    /// The status line consists of:
    /// - A top border (separator from content above)
    /// - Colored status text with optional spinner character and message
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let (text, style) = self.build_status_line();

        let paragraph = Paragraph::new(text)
            .style(style)
            .block(Block::default().borders(Borders::TOP));
        paragraph.render(area, buf);
    }

    /// Always returns 2 (1 for top border + 1 for status text).
    fn desired_height(&self, _width: u16) -> u16 {
        2
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

impl StatusBar {
    /// Build the status line text and associated style based on the current
    /// AI state.
    fn build_status_line(&self) -> (String, Style) {
        match self.status {
            AiStatus::Idle => {
                (" 等待输入 ".to_string(), Style::default().fg(Color::Green))
            }
            AiStatus::Submitting => {
                let spinner = SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()];
                let text = match &self.message {
                    Some(msg) => format!(" 提交中 {}  {} ", spinner, msg),
                    None => format!(" 提交中 {} ", spinner),
                };
                (text, Style::default().fg(Color::Cyan))
            }
            AiStatus::Thinking => {
                let spinner = SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()];
                let text = match &self.message {
                    Some(msg) => format!(" 思考中 {}  {} ", spinner, msg),
                    None => format!(" 思考中 {} ", spinner),
                };
                (text, Style::default().fg(Color::Cyan))
            }
            AiStatus::Executing => {
                let spinner = SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()];
                let text = match &self.message {
                    Some(msg) => format!(" 执行中 {}  {} ", spinner, msg),
                    None => format!(" 执行中 {} ", spinner),
                };
                (text, Style::default().fg(Color::Blue))
            }
            AiStatus::WaitingApproval => {
                let text = match &self.message {
                    Some(msg) => format!(" 等待审批 — {} ", msg),
                    None => " 等待审批 ".to_string(),
                };
                (text, Style::default().fg(Color::Yellow))
            }
            AiStatus::Interrupted => {
                let text = match &self.message {
                    Some(msg) => format!(" 已中断 — {} ", msg),
                    None => " 已中断 ".to_string(),
                };
                (text, Style::default().fg(Color::Red))
            }
            AiStatus::Error => {
                let text = match &self.message {
                    Some(msg) => format!(" 错误: {} ", msg),
                    None => " 错误 ".to_string(),
                };
                (text, Style::default().fg(Color::Red))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::cursor::SetCursorStyle;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    // -- AiStatus -----------------------------------------------------------

    #[test]
    fn test_ai_status_debug_clone_copy() {
        let status = AiStatus::Idle;
        let copied = status;
        assert_eq!(format!("{:?}", status), "Idle");
        assert_eq!(copied, status);
    }

    #[test]
    fn test_ai_status_all_variants_distinct() {
        let variants = [
            AiStatus::Idle,
            AiStatus::Submitting,
            AiStatus::Thinking,
            AiStatus::Executing,
            AiStatus::WaitingApproval,
            AiStatus::Interrupted,
            AiStatus::Error,
        ];
        for i in 0..variants.len() {
            for j in (i + 1)..variants.len() {
                assert_ne!(variants[i], variants[j]);
            }
        }
    }

    // -- Spinner frames -----------------------------------------------------

    #[test]
    fn test_spinner_frames_have_10_frames() {
        assert_eq!(SPINNER_FRAMES.len(), 10);
    }

    #[test]
    fn test_spinner_frames_are_unique_chars() {
        let mut seen = std::collections::HashSet::new();
        for &ch in SPINNER_FRAMES {
            assert!(seen.insert(ch), "duplicate frame: {}", ch);
        }
    }

    // -- StatusBar basic ----------------------------------------------------

    #[test]
    fn test_status_bar_new_is_idle() {
        let bar = StatusBar::new();
        assert_eq!(bar.status(), AiStatus::Idle);
        assert_eq!(bar.message(), None);
        assert_eq!(bar.spinner_frame, 0);
    }

    #[test]
    fn test_status_bar_default_is_idle() {
        let bar = StatusBar::default();
        assert_eq!(bar.status(), AiStatus::Idle);
    }

    #[test]
    fn test_status_bar_set_status() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Thinking);
        assert_eq!(bar.status(), AiStatus::Thinking);
    }

    #[test]
    fn test_status_bar_set_message() {
        let mut bar = StatusBar::new();
        assert_eq!(bar.message(), None);

        bar.set_message(Some("hello".to_string()));
        assert_eq!(bar.message(), Some("hello"));

        bar.set_message(None);
        assert_eq!(bar.message(), None);
    }

    #[test]
    fn test_status_bar_tick_advances_frame() {
        let mut bar = StatusBar::new();
        assert_eq!(bar.spinner_frame, 0);

        bar.tick();
        assert_eq!(bar.spinner_frame, 1);

        // Advance to the end
        for _ in 0..(SPINNER_FRAMES.len() - 1) {
            bar.tick();
        }
        assert_eq!(bar.spinner_frame, 0); // wrapped around
    }

    #[test]
    fn test_status_bar_reset() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Error);
        bar.set_message(Some("oops".to_string()));
        bar.tick(); // advance frame
        assert_ne!(bar.spinner_frame, 0);

        bar.reset();
        assert_eq!(bar.status(), AiStatus::Idle);
        assert_eq!(bar.message(), None);
        assert_eq!(bar.spinner_frame, 0);
    }

    // -- Renderable trait ---------------------------------------------------

    #[test]
    fn test_desired_height_is_always_2() {
        let bar = StatusBar::new();
        assert_eq!(bar.desired_height(80), 2);
        assert_eq!(bar.desired_height(0), 2);
        assert_eq!(bar.desired_height(200), 2);
    }

    #[test]
    fn test_render_idle_does_not_panic() {
        let bar = StatusBar::new();
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);
    }

    #[test]
    fn test_render_thinking_does_not_panic() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Thinking);
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);
    }

    #[test]
    fn test_render_executing_with_message() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Executing);
        bar.set_message(Some("bash".to_string()));
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);
    }

    #[test]
    fn test_render_waiting_approval() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::WaitingApproval);
        bar.set_message(Some("Approve?  (y/N)".to_string()));
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);
    }

    #[test]
    fn test_render_error_with_message() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Error);
        bar.set_message(Some("Connection refused".to_string()));
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);
    }

    #[test]
    fn test_render_all_states_render_nicely() {
        let states = [
            AiStatus::Idle,
            AiStatus::Submitting,
            AiStatus::Thinking,
            AiStatus::Executing,
            AiStatus::WaitingApproval,
            AiStatus::Interrupted,
            AiStatus::Error,
        ];
        let area = Rect::new(0, 0, 80, 1);
        for &status in &states {
            let mut bar = StatusBar::new();
            bar.set_status(status);
            let mut buf = Buffer::empty(area);
            bar.render(area, &mut buf);
            // Check that the status area is non-empty (first cell has content)
            let cell = buf.cell((0, 0)).expect("cell should exist");
            // The top border renders as '─' on the first line
            assert!(
                cell.symbol() == " " || cell.symbol() == "─",
                "unexpected char for {:?}: {:?}",
                status,
                cell.symbol()
            );
        }
    }

    #[test]
    fn test_render_zero_width_area() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Error);
        let area = Rect::new(0, 0, 0, 1);
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        bar.render(area, &mut buf); // No panic
    }

    #[test]
    fn test_render_zero_height_area() {
        let bar = StatusBar::new();
        let area = Rect::new(0, 0, 80, 0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        bar.render(area, &mut buf); // No panic
    }

    // =======================================================================
    // Phase 2 additions: trait defaults, boundary, state transitions, output
    // =======================================================================

    // -- Renderable trait defaults ------------------------------------------

    #[test]
    fn test_status_bar_cursor_pos_default() {
        let bar = StatusBar::new();
        let area = Rect::new(0, 0, 80, 1);
        // StatusBar does not override cursor_pos — should return None.
        assert_eq!(bar.cursor_pos(area), None);
        assert_eq!(bar.cursor_pos(Rect::new(0, 0, 0, 0)), None);
    }

    #[test]
    fn test_status_bar_cursor_style_default() {
        let bar = StatusBar::new();
        let area = Rect::new(0, 0, 80, 1);
        // StatusBar does not override cursor_style — should return DefaultUserShape.
        assert_eq!(
            bar.cursor_style(area),
            SetCursorStyle::DefaultUserShape
        );
    }

    // -- Boundary conditions ------------------------------------------------

    #[test]
    fn test_empty_string_message() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Error);
        bar.set_message(Some(String::new()));
        assert_eq!(bar.message(), Some(""));
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf); // should not panic
    }

    #[test]
    fn test_very_long_message() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Thinking);
        let long_msg = "a".repeat(10_000);
        bar.set_message(Some(long_msg));
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf); // should not panic or truncate abruptly
    }

    #[test]
    fn test_tick_wraps_repeatedly() {
        let mut bar = StatusBar::new();
        // Advance through many cycles to verify no overflow or off-by-one
        for i in 0..1000 {
            bar.tick();
            let expected = (i + 1) % SPINNER_FRAMES.len();
            assert_eq!(
                bar.spinner_frame, expected,
                "frame mismatch at tick {}",
                i + 1
            );
        }
    }

    #[test]
    fn test_render_max_width() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Thinking);
        bar.set_message(Some("test".to_string()));
        let area = Rect::new(0, 0, u16::MAX, 1);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf); // should not panic
    }

    #[test]
    fn test_tick_on_any_state() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Idle);
        bar.tick();
        assert_eq!(bar.spinner_frame, 1);

        bar.set_status(AiStatus::Error);
        bar.tick();
        assert_eq!(bar.spinner_frame, 2);
    }

    #[test]
    fn test_render_out_of_bounds_area() {
        let bar = StatusBar::new();
        // Area larger than the allocated buffer — ratatui should clip
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 1)); // smaller buffer
        bar.render(area, &mut buf); // should not panic
    }

    #[test]
    fn test_message_cleared_then_set_multiple() {
        let mut bar = StatusBar::new();
        assert_eq!(bar.message(), None);

        bar.set_message(Some("first".to_string()));
        assert_eq!(bar.message(), Some("first"));

        bar.set_message(Some("second".to_string()));
        assert_eq!(bar.message(), Some("second"));

        bar.set_message(None);
        assert_eq!(bar.message(), None);

        bar.set_message(Some("third".to_string()));
        assert_eq!(bar.message(), Some("third"));
    }

    // -- State transitions --------------------------------------------------

    #[test]
    fn test_status_full_cycle() {
        let mut bar = StatusBar::new();
        assert_eq!(bar.status(), AiStatus::Idle);

        bar.set_status(AiStatus::Thinking);
        assert_eq!(bar.status(), AiStatus::Thinking);

        bar.set_status(AiStatus::Executing);
        assert_eq!(bar.status(), AiStatus::Executing);

        bar.set_status(AiStatus::WaitingApproval);
        assert_eq!(bar.status(), AiStatus::WaitingApproval);

        bar.set_status(AiStatus::Error);
        assert_eq!(bar.status(), AiStatus::Error);

        // Reset should bring everything back to initial state
        bar.reset();
        assert_eq!(bar.status(), AiStatus::Idle);
        assert_eq!(bar.message(), None);
        assert_eq!(bar.spinner_frame, 0);
    }

    #[test]
    fn test_status_round_trip() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Error);
        assert_eq!(bar.status(), AiStatus::Error);
        bar.set_status(AiStatus::Idle);
        assert_eq!(bar.status(), AiStatus::Idle);
    }

    #[test]
    fn test_set_same_status_repeatedly() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Thinking);
        bar.set_status(AiStatus::Thinking);
        bar.set_status(AiStatus::Thinking);
        assert_eq!(bar.status(), AiStatus::Thinking);
    }

    #[test]
    fn test_message_persists_through_status_change() {
        let mut bar = StatusBar::new();
        bar.set_message(Some("hello".to_string()));

        bar.set_status(AiStatus::Thinking);
        assert_eq!(bar.message(), Some("hello"));

        bar.set_status(AiStatus::Executing);
        assert_eq!(bar.message(), Some("hello"));

        bar.set_status(AiStatus::Error);
        assert_eq!(bar.message(), Some("hello"));
    }

    #[test]
    fn test_then_set_status_after_tick() {
        let mut bar = StatusBar::new();
        bar.tick(); // frame = 1
        bar.set_status(AiStatus::Thinking);
        assert_eq!(bar.status(), AiStatus::Thinking);
        assert_eq!(bar.spinner_frame, 1); // tick state preserved
    }

    #[test]
    fn test_reset_then_reuse() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Error);
        bar.set_message(Some("err".to_string()));
        bar.tick();
        bar.tick();
        bar.reset();

        // Reuse after reset — should work like a fresh StatusBar
        bar.set_status(AiStatus::Executing);
        bar.set_message(Some("new task".to_string()));
        assert_eq!(bar.status(), AiStatus::Executing);
        assert_eq!(bar.message(), Some("new task"));
        assert_eq!(bar.spinner_frame, 0);
    }

    #[test]
    fn test_multiple_ticks_after_reset() {
        let mut bar = StatusBar::new();
        bar.tick();
        bar.tick();
        bar.reset();
        assert_eq!(bar.spinner_frame, 0);

        bar.tick();
        assert_eq!(bar.spinner_frame, 1);

        bar.tick();
        assert_eq!(bar.spinner_frame, 2);
    }

    // -- Render output verification -----------------------------------------

    #[test]
    fn test_render_idle_contains_label() {
        let bar = StatusBar::new();
        // Use 2 rows because Borders::TOP occupies row 0, text lives on row 1.
        let area = Rect::new(0, 0, 80, 2);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);

        // Row 0 is the top border
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("─"));
        // Row 1 contains the idle label — Chinese chars are full-width in
        // ratatui's buffer, so they appear as individual symbols separated
        // by empty cells. Check each character independently.
        let cells: String = (0..area.width)
            .filter_map(|x| buf.cell((x, 1)).map(|c| c.symbol()))
            .collect();
        assert!(cells.contains("等"), "idle char '等' not found in: {:?}", cells);
        assert!(cells.contains("待"), "idle char '待' not found in: {:?}", cells);
        assert!(cells.contains("输"), "idle char '输' not found in: {:?}", cells);
        assert!(cells.contains("入"), "idle char '入' not found in: {:?}", cells);
    }

    #[test]
    fn test_render_thinking_contains_spinner() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Thinking);
        // Use 2 rows because Borders::TOP occupies row 0, text lives on row 1.
        let area = Rect::new(0, 0, 80, 2);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);

        let cells: String = (0..area.width)
            .filter_map(|x| buf.cell((x, 1)).map(|c| c.symbol()))
            .collect();
        // The spinner should be the first frame: '⠋'
        assert!(cells.contains('⠋'), "spinner char not found in output");
    }

    #[test]
    fn test_render_executing_contains_spinner() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Executing);
        // Use 2 rows because Borders::TOP occupies row 0, text lives on row 1.
        let area = Rect::new(0, 0, 80, 2);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);

        let cells: String = (0..area.width)
            .filter_map(|x| buf.cell((x, 1)).map(|c| c.symbol()))
            .collect();
        assert!(cells.contains('⠋'), "spinner char not found in output");
    }

    #[test]
    fn test_render_error_contains_message() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Error);
        bar.set_message(Some("something went wrong".to_string()));
        // Use 2 rows because Borders::TOP occupies row 0, text lives on row 1.
        let area = Rect::new(0, 0, 80, 2);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);

        let cells: String = (0..area.width)
            .filter_map(|x| buf.cell((x, 1)).map(|c| c.symbol()))
            .collect();
        // Chinese chars are full-width in ratatui's buffer — check individually.
        assert!(cells.contains("错"), "error char '错' not found in: {:?}", cells);
        assert!(cells.contains("误"), "error char '误' not found in: {:?}", cells);
        // ASCII message text is contiguous.
        assert!(
            cells.contains("something went wrong"),
            "error message not found in: {:?}",
            cells
        );
    }

    #[test]
    fn test_render_thinking_contains_message() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::Thinking);
        bar.set_message(Some("Generating...".to_string()));
        // Use 2 rows because Borders::TOP occupies row 0, text lives on row 1.
        let area = Rect::new(0, 0, 80, 2);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);

        let cells: String = (0..area.width)
            .filter_map(|x| buf.cell((x, 1)).map(|c| c.symbol()))
            .collect();
        assert!(
            cells.contains("Generating..."),
            "message not found in: {:?}",
            cells
        );
    }

    #[test]
    fn test_render_waiting_approval_contains_message() {
        let mut bar = StatusBar::new();
        bar.set_status(AiStatus::WaitingApproval);
        bar.set_message(Some("Approve? (y/N)".to_string()));
        // Use 2 rows because Borders::TOP occupies row 0, text lives on row 1.
        let area = Rect::new(0, 0, 80, 2);
        let mut buf = Buffer::empty(area);
        bar.render(area, &mut buf);

        let cells: String = (0..area.width)
            .filter_map(|x| buf.cell((x, 1)).map(|c| c.symbol()))
            .collect();
        // Chinese chars are full-width in ratatui's buffer — check individually.
        assert!(cells.contains("等"), "approval char '等' not found in: {:?}", cells);
        assert!(cells.contains("待"), "approval char '待' not found in: {:?}", cells);
        assert!(cells.contains("审"), "approval char '审' not found in: {:?}", cells);
        assert!(cells.contains("批"), "approval char '批' not found in: {:?}", cells);
        // ASCII message text is contiguous.
        assert!(
            cells.contains("Approve?"),
            "approval message not found in: {:?}",
            cells
        );
    }

    #[test]
    fn test_render_all_states_without_message() {
        let states = [
            AiStatus::Idle,
            AiStatus::Thinking,
            AiStatus::Executing,
            AiStatus::WaitingApproval,
            AiStatus::Error,
        ];
        let area = Rect::new(0, 0, 80, 1);
        for &status in &states {
            let mut bar = StatusBar::new();
            bar.set_status(status);
            // No message set — should not panic
            let mut buf = Buffer::empty(area);
            bar.render(area, &mut buf);
        }
    }
}