//! Spinner animation adapter for the TUI rendering system.
//!
//! Provides a frame-based animated spinner using braille dots characters.
//! Implements the [`Renderable`] trait so it can be placed directly into the
//! TUI layout (e.g. inside a [`StatusBar`](crate::tui::status::StatusBar)).
//!
//! # Example
//!
//! ```ignore
//! let mut spinner = SpinnerWidget::new();
//! spinner.tick();               // advance one frame
//! let frame = spinner.current_frame(); // "⠙" (after first tick)
//! ```
//!
//! # Animation frames
//!
//! The default frame set is the classic braille dots spinner:
//! `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`

use crate::tui::render::Renderable;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
};

/// A frame-based spinner animation widget.
///
/// Cycles through a sequence of braille dots characters on every [`tick()`].
/// Renders the current frame as a single character at the top-left of the
/// allocated area.
///
/// [`tick()`]: Self::tick
pub struct SpinnerWidget {
    /// Index into [`frames`] for the current animation frame.
    frame_index: usize,
    /// The ordered list of braille-dot frame characters.
    frames: Vec<&'static str>,
}

impl SpinnerWidget {
    /// Create a new spinner with the default 10-frame dots animation.
    ///
    /// The frames are: `⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏`
    #[must_use]
    pub fn new() -> Self {
        Self {
            frame_index: 0,
            frames: vec![
                "⠋", "⠙", "⠹", "⠸", "⠼",
                "⠴", "⠦", "⠧", "⠇", "⠏",
            ],
        }
    }

    /// Create a spinner with a custom set of frame strings.
    ///
    /// The caller is responsible for ensuring `frames` is non-empty.
    #[must_use]
    pub fn with_frames(frames: Vec<&'static str>) -> Self {
        Self {
            frame_index: 0,
            frames,
        }
    }

    /// Advance the spinner to the next frame.
    ///
    /// Call this periodically (e.g. every 100 ms from the event loop) to
    /// produce a smooth animation.
    pub fn tick(&mut self) {
        if !self.frames.is_empty() {
            self.frame_index = (self.frame_index + 1) % self.frames.len();
        }
    }

    /// Return the current frame character as a string slice.
    #[must_use]
    pub fn current_frame(&self) -> &str {
        if self.frames.is_empty() {
            " "
        } else {
            self.frames[self.frame_index]
        }
    }

    /// Reset the spinner to the first frame.
    pub fn reset(&mut self) {
        self.frame_index = 0;
    }
}

impl Default for SpinnerWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderable for SpinnerWidget {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let frame = self.current_frame();
        if let Some(cell) = buf.cell_mut((area.x, area.y)) {
            cell.set_symbol(frame);
        }
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    // -----------------------------------------------------------------------
    // SpinnerWidget construction
    // -----------------------------------------------------------------------

    #[test]
    fn test_new_has_ten_frames() {
        let spinner = SpinnerWidget::new();
        assert_eq!(spinner.frames.len(), 10);
        assert_eq!(spinner.frame_index, 0);
    }

    #[test]
    fn test_with_frames() {
        let frames = vec!["a", "b", "c"];
        let spinner = SpinnerWidget::with_frames(frames);
        assert_eq!(spinner.frames.len(), 3);
        assert_eq!(spinner.current_frame(), "a");
    }

    #[test]
    fn test_default_is_new() {
        let a = SpinnerWidget::new();
        let b = SpinnerWidget::default();
        assert_eq!(a.frames, b.frames);
        assert_eq!(a.frame_index, b.frame_index);
    }

    // -----------------------------------------------------------------------
    // tick / current_frame
    // -----------------------------------------------------------------------

    #[test]
    fn test_tick_advances_frame() {
        let mut spinner = SpinnerWidget::new();
        assert_eq!(spinner.current_frame(), "⠋");

        spinner.tick();
        assert_eq!(spinner.current_frame(), "⠙");

        spinner.tick();
        assert_eq!(spinner.current_frame(), "⠹");
    }

    #[test]
    fn test_tick_wraps_around() {
        let frames = vec!["x", "y"];
        let mut spinner = SpinnerWidget::with_frames(frames);

        assert_eq!(spinner.current_frame(), "x");
        spinner.tick();
        assert_eq!(spinner.current_frame(), "y");
        spinner.tick();
        assert_eq!(spinner.current_frame(), "x"); // wraps
    }

    #[test]
    fn test_reset_goes_to_zero() {
        let mut spinner = SpinnerWidget::new();
        spinner.tick();
        spinner.tick();
        assert_ne!(spinner.frame_index, 0);

        spinner.reset();
        assert_eq!(spinner.frame_index, 0);
        assert_eq!(spinner.current_frame(), "⠋");
    }

    #[test]
    fn test_current_frame_empty_frames() {
        let spinner = SpinnerWidget::with_frames(vec![]);
        assert_eq!(spinner.current_frame(), " ");
    }

    #[test]
    fn test_tick_no_crash_on_empty() {
        let mut spinner = SpinnerWidget::with_frames(vec![]);
        spinner.tick(); // no crash
        assert_eq!(spinner.frame_index, 0);
    }

    // -----------------------------------------------------------------------
    // Renderable
    // -----------------------------------------------------------------------

    #[test]
    fn test_desired_height_is_one() {
        let spinner = SpinnerWidget::new();
        assert_eq!(spinner.desired_height(80), 1);
    }

    #[test]
    fn test_render_places_frame_at_origin() {
        let spinner = SpinnerWidget::new();
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);

        spinner.render(area, &mut buf);

        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("⠋"));
    }

    #[test]
    fn test_render_empty_area_no_panic() {
        let spinner = SpinnerWidget::new();
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        spinner.render(area, &mut buf); // no panic
    }

    #[test]
    fn test_render_after_tick() {
        let mut spinner = SpinnerWidget::new();
        spinner.tick();

        let area = Rect::new(5, 3, 10, 1);
        let mut buf = Buffer::empty(area);

        spinner.render(area, &mut buf);

        assert_eq!(buf.cell((5, 3)).map(|c| c.symbol()), Some("⠙"));
    }

    // -----------------------------------------------------------------------
    // Frame cycle completeness
    // -----------------------------------------------------------------------

    #[test]
    fn test_spinner_frame_cycle() {
        let mut spinner = SpinnerWidget::new();
        let mut seen = std::collections::HashSet::new();

        for _ in 0..spinner.frames.len() {
            let frame = spinner.current_frame().to_string();
            seen.insert(frame);
            spinner.tick();
        }

        assert_eq!(seen.len(), 10, "all 10 unique frames should be visited");
    }

    // -----------------------------------------------------------------------
    // Renderable trait method defaults (cursor_pos / cursor_style)
    // -----------------------------------------------------------------------

    #[test]
    fn test_cursor_pos_default() {
        let spinner = SpinnerWidget::new();
        let area = Rect::new(0, 0, 80, 1);
        assert_eq!(spinner.cursor_pos(area), None);
    }

    #[test]
    fn test_cursor_style_default() {
        let spinner = SpinnerWidget::new();
        let area = Rect::new(0, 0, 80, 1);
        assert_eq!(
            spinner.cursor_style(area),
            crossterm::cursor::SetCursorStyle::DefaultUserShape,
        );
    }

    // -----------------------------------------------------------------------
    // Boundary conditions
    // -----------------------------------------------------------------------

    #[test]
    fn test_with_frames_single_frame() {
        let mut spinner = SpinnerWidget::with_frames(vec!["*"]);
        assert_eq!(spinner.current_frame(), "*");
        // tick on a single-frame spinner stays on the same frame
        spinner.tick();
        assert_eq!(spinner.current_frame(), "*");
        spinner.tick();
        assert_eq!(spinner.current_frame(), "*");
        assert_eq!(spinner.frame_index, 0, "single-frame index stays 0");
    }

    #[test]
    fn test_tick_wraps_exactly_at_boundary() {
        let frames = vec!["a", "b", "c"];
        let mut spinner = SpinnerWidget::with_frames(frames);
        assert_eq!(spinner.current_frame(), "a");
        spinner.tick();
        assert_eq!(spinner.current_frame(), "b");
        spinner.tick();
        assert_eq!(spinner.current_frame(), "c");
        // 3rd tick wraps back to index 0
        spinner.tick();
        assert_eq!(spinner.current_frame(), "a");
    }

    #[test]
    fn test_tick_many_cycles_no_overflow() {
        let mut spinner = SpinnerWidget::new();
        // 10 000 ticks — should not panic or degrade
        for _ in 0..10_000 {
            spinner.tick();
        }
        // 10 000 % 10 == 0, so we're back at frame 0
        assert_eq!(spinner.current_frame(), "⠋");
        assert_eq!(spinner.frame_index, 0);
    }

    #[test]
    fn test_render_zero_width_skips() {
        let spinner = SpinnerWidget::new();
        let area = Rect::new(0, 0, 0, 1);
        let mut buf = Buffer::empty(area);
        spinner.render(area, &mut buf); // no panic, no-op
    }

    #[test]
    fn test_render_zero_height_skips() {
        let spinner = SpinnerWidget::new();
        let area = Rect::new(0, 0, 10, 0);
        let mut buf = Buffer::empty(area);
        spinner.render(area, &mut buf); // no panic, no-op
    }

    #[test]
    fn test_render_at_non_zero_origin() {
        let spinner = SpinnerWidget::new();
        let full_area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(full_area);
        let area = Rect::new(5, 2, 10, 1);
        spinner.render(area, &mut buf);
        // Frame should be at (5, 2)
        assert_eq!(buf.cell((5, 2)).map(|c| c.symbol()), Some("⠋"));
        // (0, 0) should remain untouched (default space)
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some(" "));
    }

    #[test]
    fn test_render_custom_frame() {
        let spinner = SpinnerWidget::with_frames(vec!["🔵"]);
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        spinner.render(area, &mut buf);
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("🔵"));
    }

    #[test]
    fn test_render_out_of_bounds_area() {
        // Area whose coordinates fall outside the buffer — cell_mut returns
        // None, which should be handled gracefully (no panic).
        let spinner = SpinnerWidget::new();
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        let outside = Rect::new(100, 100, 10, 1);
        spinner.render(outside, &mut buf); // no panic, graceful no-op
    }

    // -----------------------------------------------------------------------
    // State transitions
    // -----------------------------------------------------------------------

    #[test]
    fn test_tick_then_reset_then_tick() {
        let mut spinner = SpinnerWidget::new();
        spinner.tick();
        spinner.tick();
        assert_eq!(spinner.current_frame(), "⠹", "after 2 ticks");
        spinner.reset();
        assert_eq!(spinner.current_frame(), "⠋", "back to first frame");
        spinner.tick();
        assert_eq!(spinner.current_frame(), "⠙", "advances again after reset");
    }

    #[test]
    fn test_reset_idempotent() {
        let mut spinner = SpinnerWidget::new();
        spinner.reset();
        assert_eq!(spinner.frame_index, 0);
        spinner.reset();
        assert_eq!(spinner.frame_index, 0);
        assert_eq!(spinner.current_frame(), "⠋");
    }

    // -----------------------------------------------------------------------
    // desired_height edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_desired_height_independent_of_width() {
        let spinner = SpinnerWidget::new();
        assert_eq!(spinner.desired_height(0), 1);
        assert_eq!(spinner.desired_height(u16::MAX), 1);
        assert_eq!(spinner.desired_height(1), 1);
        assert_eq!(spinner.desired_height(80), 1);
    }

    // -----------------------------------------------------------------------
    // Empty frames: render behaviour
    // -----------------------------------------------------------------------

    #[test]
    fn test_render_empty_frames_shows_space() {
        let spinner = SpinnerWidget::with_frames(vec![]);
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        spinner.render(area, &mut buf);
        // current_frame() returns " " when frames is empty
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some(" "));
    }
}