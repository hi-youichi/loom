//! Inline viewport management for the TUI.
//!
//! Manages a fixed-size rectangular region within the terminal's normal
//! scrollback, positioned at or near the bottom of the screen. This is the
//! "chat area" where the TUI renders its content while the rest of the
//! terminal scrolls naturally.

/// Manages the inline viewport position and dimensions.
///
/// The viewport is a rectangular region rendered at a fixed position within
/// the terminal's normal scrollback (not alternate screen). It "floats" at
/// the bottom of the terminal, moving up as new history lines are inserted
/// above it.
#[derive(Debug, Clone)]
pub struct Viewport {
    /// Top row of the viewport (0-based, relative to terminal top)
    top: u16,
    /// Height of the viewport in rows
    height: u16,
    /// Width of the viewport in columns
    width: u16,
    /// Total screen height (rows)
    screen_height: u16,
    /// Whether the viewport is anchored to the bottom of the screen
    bottom_aligned: bool,
}

impl Viewport {
    /// Create a new viewport starting just below the cursor.
    ///
    /// `cursor_row` is the 0-based row where the cursor was when the TUI
    /// started. The viewport is placed at `cursor_row + 1` as the initial
    /// top, with a default height of 10 rows.
    pub fn new(cursor_row: u16, screen_width: u16, screen_height: u16) -> Self {
        let initial_top = cursor_row.saturating_add(1);
        let initial_height = 10u16.min(screen_height.saturating_sub(initial_top));

        Self {
            top: initial_top,
            height: initial_height,
            width: screen_width,
            screen_height,
            bottom_aligned: false,
        }
    }

    /// Get the viewport's top row (0-based).
    pub fn top(&self) -> u16 {
        self.top
    }

    /// Get the viewport height.
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Get the viewport width.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Get the viewport size as `(width, height)`.
    pub fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    /// Get the screen height.
    pub fn screen_height(&self) -> u16 {
        self.screen_height
    }

    /// Set the viewport height.
    pub fn set_height(&mut self, height: u16) {
        self.height = height.min(self.screen_height);
        if self.bottom_aligned {
            self.top = self.screen_height.saturating_sub(self.height);
        }
    }

    /// Check if the viewport is bottom-aligned.
    pub fn is_bottom_aligned(&self) -> bool {
        self.bottom_aligned
    }

    /// Set whether the viewport is anchored to the bottom of the screen.
    ///
    /// When bottom-aligned, `top` is automatically adjusted to keep the
    /// viewport at the bottom of the terminal.
    pub fn set_bottom_aligned(&mut self, aligned: bool) {
        self.bottom_aligned = aligned;
        if aligned {
            self.top = self.screen_height.saturating_sub(self.height);
        }
    }

    /// Handle terminal resize.
    ///
    /// Returns `true` if the viewport dimensions changed and a full redraw
    /// is needed.
    pub fn handle_resize(&mut self, new_width: u16, new_height: u16) -> bool {
        let old_width = self.width;
        let old_screen_height = self.screen_height;

        self.width = new_width;
        self.screen_height = new_height;

        // If bottom-aligned, adjust top to keep bottom edge fixed
        if self.bottom_aligned {
            self.top = new_height.saturating_sub(self.height);
        } else {
            // Clamp top to ensure viewport fits within screen
            if self.top.saturating_add(self.height) > new_height {
                self.top = new_height.saturating_sub(self.height);
            }
        }

        old_width != new_width || old_screen_height != new_height
    }

    /// Move the viewport up by `delta` rows (e.g., after inserting history lines).
    ///
    /// Returns the actual number of rows moved (may be less if at top of screen).
    pub fn scroll_up(&mut self, delta: u16) -> u16 {
        let actual = delta.min(self.top);
        self.top -= actual;
        actual
    }

    /// Get the bottom row of the viewport (0-based, exclusive).
    pub fn bottom(&self) -> u16 {
        self.top.saturating_add(self.height)
    }

    /// Check if the viewport fits entirely within the screen.
    pub fn fits_in_screen(&self) -> bool {
        self.top.saturating_add(self.height) <= self.screen_height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_new() {
        let vp = Viewport::new(5, 80, 24);
        assert_eq!(vp.top(), 6);
        assert_eq!(vp.height(), 10);
        assert_eq!(vp.width(), 80);
        assert_eq!(vp.screen_height(), 24);
        assert!(!vp.is_bottom_aligned());
    }

    #[test]
    fn test_viewport_new_cursor_at_bottom() {
        let vp = Viewport::new(22, 80, 24);
        assert_eq!(vp.top(), 23);
        assert_eq!(vp.height(), 1); // only 1 row left
    }

    #[test]
    fn test_viewport_bottom_aligned() {
        let mut vp = Viewport::new(5, 80, 24);
        vp.set_bottom_aligned(true);
        assert!(vp.is_bottom_aligned());
        assert_eq!(vp.top(), 14); // 24 - 10 = 14
    }

    #[test]
    fn test_viewport_resize() {
        let mut vp = Viewport::new(5, 80, 24);
        vp.set_bottom_aligned(true);

        let changed = vp.handle_resize(120, 30);
        assert!(changed);
        assert_eq!(vp.width(), 120);
        assert_eq!(vp.screen_height(), 30);
        // bottom-aligned: top = 30 - 10 = 20
        assert_eq!(vp.top(), 20);
    }

    #[test]
    fn test_viewport_scroll_up() {
        let mut vp = Viewport::new(5, 80, 24);
        let moved = vp.scroll_up(3);
        assert_eq!(moved, 3);
        assert_eq!(vp.top(), 3);
    }

    #[test]
    fn test_viewport_scroll_up_at_top() {
        let mut vp = Viewport::new(0, 80, 24);
        let moved = vp.scroll_up(5);
        assert_eq!(moved, 1); // top starts at 1, can move up by 1 to 0
        assert_eq!(vp.top(), 0);
    }

    #[test]
    fn test_viewport_fits() {
        let vp = Viewport::new(5, 80, 24);
        assert!(vp.fits_in_screen());
    }

    #[test]
    fn test_viewport_bottom() {
        let vp = Viewport::new(5, 80, 24);
        assert_eq!(vp.bottom(), 16); // 6 + 10
    }
}