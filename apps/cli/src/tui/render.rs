//! Renderable trait and layout components.
//!
//! Core rendering abstraction for all TUI UI components.
//!
//! # Overview
//!
//! - [`Renderable`] — the trait every visual component implements.
//! - [`ColumnRenderable`] — vertical stack of children, each allocated its
//!   `desired_height()`.
//! - [`FlexRenderable`] — proportional space distribution by weight.
//! - [`InsetRenderable`] — padding/margin wrapper.

use crossterm::cursor::SetCursorStyle;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
};

/// All UI components must implement this trait for rendering.
///
/// The layout system calls `desired_height()` to determine how much vertical
/// space a component needs, then calls `render()` to draw it. Two optional
/// methods, `cursor_pos()` and `cursor_style()`, are used by input components
/// such as text fields.
pub trait Renderable {
    /// Render to a ratatui buffer within the given area.
    fn render(&self, area: Rect, buf: &mut Buffer);

    /// Tell the layout system how many rows this component needs.
    fn desired_height(&self, width: u16) -> u16;

    /// Cursor position (for input fields). `None` if no cursor.
    fn cursor_pos(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }

    /// Cursor style when this component is the active input target.
    fn cursor_style(&self, _area: Rect) -> SetCursorStyle {
        SetCursorStyle::DefaultUserShape
    }
}

// ---------------------------------------------------------------------------
// ColumnRenderable — vertical stacking
// ---------------------------------------------------------------------------

/// A vertical stack of renderable children.
///
/// Children are rendered top-to-bottom. Each child receives exactly its
/// `desired_height()` at the given width, clipped to the remaining space.
///
/// # Example
///
/// ```ignore
/// let column = ColumnRenderable::new(vec![&header, &content, &footer]);
/// column.render(area, buf);
/// ```
pub struct ColumnRenderable<'a> {
    children: Vec<&'a dyn Renderable>,
}

impl<'a> ColumnRenderable<'a> {
    /// Create a new column from a list of children.
    pub fn new(children: Vec<&'a dyn Renderable>) -> Self {
        Self { children }
    }

    /// Append a child to the end of the column.
    pub fn push(&mut self, child: &'a dyn Renderable) {
        self.children.push(child);
    }

    /// Number of children.
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Whether the column has no children.
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

impl<'a> Renderable for ColumnRenderable<'a> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut y = area.y;
        for child in &self.children {
            let height = child
                .desired_height(area.width)
                .min(area.height.saturating_sub(y - area.y));
            let child_area = Rect::new(area.x, y, area.width, height);
            child.render(child_area, buf);
            y += height;
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.children.iter().map(|c| c.desired_height(width)).sum()
    }
}

impl<'a> FromIterator<&'a dyn Renderable> for ColumnRenderable<'a> {
    fn from_iter<I: IntoIterator<Item = &'a dyn Renderable>>(iter: I) -> Self {
        Self {
            children: iter.into_iter().collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// FlexRenderable — proportional space distribution
// ---------------------------------------------------------------------------

/// A flex layout that distributes vertical space proportionally.
///
/// Each child is paired with a weight (`u16`). The total available height
/// is divided according to `weight / total_weight`.
///
/// `desired_height()` returns 0 because flex children claim all available
/// space — the caller must allocate the area.
///
/// # Example
///
/// ```ignore
/// let flex = FlexRenderable::new(vec![
///     (&header, 1),   // 1/4
///     (&content, 2),  // 2/4
///     (&footer, 1),   // 1/4
/// ]);
/// flex.render(area, buf);
/// ```
pub struct FlexRenderable<'a> {
    children: Vec<(&'a dyn Renderable, u16)>,
}

impl<'a> FlexRenderable<'a> {
    /// Create a new flex layout from `(child, weight)` pairs.
    pub fn new(children: Vec<(&'a dyn Renderable, u16)>) -> Self {
        Self { children }
    }

    /// Append a child with a given weight.
    pub fn push(&mut self, child: &'a dyn Renderable, weight: u16) {
        self.children.push((child, weight));
    }

    /// Number of children.
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Whether the flex layout has no children.
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

impl<'a> Renderable for FlexRenderable<'a> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let total_weight: u16 = self.children.iter().map(|(_, w)| w).sum();
        if total_weight == 0 {
            return;
        }
        let mut y = area.y;
        for (child, weight) in &self.children {
            let height = area.height * weight / total_weight;
            let child_area = Rect::new(area.x, y, area.width, height);
            child.render(child_area, buf);
            y += height;
        }
    }

    fn desired_height(&self, _width: u16) -> u16 {
        // Flex layout claims all available space; the caller allocates it.
        0
    }
}

// ---------------------------------------------------------------------------
// InsetRenderable — padding wrapper
// ---------------------------------------------------------------------------

/// A padding wrapper that insets the inner renderable.
///
/// Shrinks the rendering area by the specified margins before delegating to
/// the inner component. The `desired_height()` is adjusted accordingly.
///
/// # Example
///
/// ```ignore
/// let padded = InsetRenderable::new(&content)
///     .with_top(1)
///     .with_bottom(1)
///     .with_left(2)
///     .with_right(2);
/// padded.render(area, buf);
/// ```
pub struct InsetRenderable<'a> {
    inner: &'a dyn Renderable,
    top: u16,
    bottom: u16,
    left: u16,
    right: u16,
}

impl<'a> InsetRenderable<'a> {
    /// Create a new inset wrapper with zero padding.
    pub fn new(inner: &'a dyn Renderable) -> Self {
        Self {
            inner,
            top: 0,
            bottom: 0,
            left: 0,
            right: 0,
        }
    }

    /// Set the top padding.
    pub fn with_top(mut self, top: u16) -> Self {
        self.top = top;
        self
    }

    /// Set the bottom padding.
    pub fn with_bottom(mut self, bottom: u16) -> Self {
        self.bottom = bottom;
        self
    }

    /// Set the left padding.
    pub fn with_left(mut self, left: u16) -> Self {
        self.left = left;
        self
    }

    /// Set the right padding.
    pub fn with_right(mut self, right: u16) -> Self {
        self.right = right;
        self
    }

    /// Set all four paddings to the same value.
    pub fn with_all(mut self, padding: u16) -> Self {
        self.top = padding;
        self.bottom = padding;
        self.left = padding;
        self.right = padding;
        self
    }
}

impl<'a> Renderable for InsetRenderable<'a> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let inner_area = Rect::new(
            area.x + self.left,
            area.y + self.top,
            area.width.saturating_sub(self.left + self.right),
            area.height.saturating_sub(self.top + self.bottom),
        );
        self.inner.render(inner_area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.inner
            .desired_height(width.saturating_sub(self.left + self.right))
            + self.top
            + self.bottom
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A minimal renderable that fills its area with a single character.
///
/// Useful for testing layouts and as a placeholder during development.
pub struct FillCell {
    ch: char,
    height: u16,
}

impl FillCell {
    /// Create a fill cell with the given character and desired height.
    pub fn new(ch: char, height: u16) -> Self {
        Self { ch, height }
    }
}

impl Renderable for FillCell {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(self.ch);
                }
            }
        }
    }

    fn desired_height(&self, _width: u16) -> u16 {
        self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    // -----------------------------------------------------------------------
    // ColumnRenderable
    // -----------------------------------------------------------------------

    #[test]
    fn test_column_desired_height() {
        let a = FillCell::new('A', 2);
        let b = FillCell::new('B', 3);
        let column = ColumnRenderable::new(vec![&a, &b]);
        assert_eq!(column.desired_height(80), 5);
    }

    #[test]
    fn test_column_clips_to_area() {
        let a = FillCell::new('A', 10);
        let b = FillCell::new('B', 10);
        let column = ColumnRenderable::new(vec![&a, &b]);

        let area = Rect::new(0, 0, 80, 5);
        let mut buf = Buffer::empty(area);
        column.render(area, &mut buf);

        // A fills the 5 available lines; B gets nothing.
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("A"));
        assert_eq!(buf.cell((0, 4)).map(|c| c.symbol()), Some("A"));
    }

    #[test]
    fn test_column_empty() {
        let column = ColumnRenderable::<'_>::new(vec![]);
        assert!(column.is_empty());
        assert_eq!(column.len(), 0);
        assert_eq!(column.desired_height(80), 0);
    }

    #[test]
    fn test_column_push() {
        let a = FillCell::new('A', 1);
        let mut column = ColumnRenderable::new(vec![&a]);
        assert_eq!(column.len(), 1);

        let b = FillCell::new('B', 2);
        column.push(&b);
        assert_eq!(column.len(), 2);
        assert_eq!(column.desired_height(80), 3);
    }

    #[test]
    fn test_column_from_iterator() {
        let a = FillCell::new('A', 1);
        let b = FillCell::new('B', 2);
        let column: ColumnRenderable =
            vec![&a as &dyn Renderable, &b].into_iter().collect();
        assert_eq!(column.len(), 2);
        assert_eq!(column.desired_height(80), 3);
    }

    // -----------------------------------------------------------------------
    // FlexRenderable
    // -----------------------------------------------------------------------

    #[test]
    fn test_flex_proportional() {
        let a = FillCell::new('A', 0);
        let b = FillCell::new('B', 0);
        let flex = FlexRenderable::new(vec![(&a, 1), (&b, 3)]);

        // Flex always reports 0 desired height.
        assert_eq!(flex.desired_height(80), 0);

        let area = Rect::new(0, 0, 80, 8);
        let mut buf = Buffer::empty(area);
        flex.render(area, &mut buf);

        // A gets 1/4 = 2 lines, B gets 3/4 = 6 lines.
        for y in 0..2 {
            assert_eq!(buf.cell((0, y)).map(|c| c.symbol()), Some("A"));
        }
        for y in 2..8 {
            assert_eq!(buf.cell((0, y)).map(|c| c.symbol()), Some("B"));
        }
    }

    #[test]
    fn test_flex_zero_weight_no_crash() {
        let flex = FlexRenderable::<'_>::new(vec![]);
        let area = Rect::new(0, 0, 80, 8);
        let mut buf = Buffer::empty(area);
        flex.render(area, &mut buf); // no crash

        let a = FillCell::new('A', 0);
        let flex2 = FlexRenderable::new(vec![(&a, 0)]);
        flex2.render(area, &mut buf); // no crash (total_weight == 0)
    }

    #[test]
    fn test_flex_push_and_empty() {
        let mut flex = FlexRenderable::new(vec![]);
        assert!(flex.is_empty());
        assert_eq!(flex.len(), 0);

        let a = FillCell::new('A', 0);
        flex.push(&a, 1);
        assert!(!flex.is_empty());
        assert_eq!(flex.len(), 1);
    }

    // -----------------------------------------------------------------------
    // InsetRenderable
    // -----------------------------------------------------------------------

    #[test]
    fn test_inset_desired_height() {
        let inner = FillCell::new('X', 5);
        let inset = InsetRenderable::new(&inner)
            .with_top(1)
            .with_bottom(1)
            .with_left(2)
            .with_right(2);

        // desired_height: inner(5) + top(1) + bottom(1) = 7
        assert_eq!(inset.desired_height(80), 7);
    }

    #[test]
    fn test_inset_renders_at_offset() {
        let inner = FillCell::new('X', 5);
        let inset = InsetRenderable::new(&inner)
            .with_top(1)
            .with_bottom(1)
            .with_left(2)
            .with_right(2);

        let area = Rect::new(0, 0, 10, 7);
        let mut buf = Buffer::empty(area);
        inset.render(area, &mut buf);

        // (0,0) should be padding (space)
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some(" "));
        // (2,1) should be the inner area
        assert_eq!(buf.cell((2, 1)).map(|c| c.symbol()), Some("X"));
    }

    #[test]
    fn test_inset_with_all() {
        let inner = FillCell::new('X', 3);
        let inset = InsetRenderable::new(&inner).with_all(2);

        assert_eq!(inset.desired_height(80), 7); // 3 + 2 + 2
    }

    // -----------------------------------------------------------------------
    // Renderable trait defaults
    // -----------------------------------------------------------------------

    #[test]
    fn test_trait_default_methods() {
        struct Minimal;
        impl Renderable for Minimal {
            fn render(&self, _area: Rect, _buf: &mut Buffer) {}
            fn desired_height(&self, _width: u16) -> u16 {
                0
            }
        }

        let m = Minimal;
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(m.cursor_pos(area), None);
        assert_eq!(m.cursor_style(area), SetCursorStyle::DefaultUserShape);
    }

    // -----------------------------------------------------------------------
    // FillCell
    // -----------------------------------------------------------------------

    #[test]
    fn test_fill_cell_render_fills_area() {
        let fill = FillCell::new('█', 3);
        let area = Rect::new(0, 0, 4, 3);
        let mut buf = Buffer::empty(area);
        fill.render(area, &mut buf);

        // Every cell in the 4×3 area should be '█'
        for y in 0..3 {
            for x in 0..4 {
                assert_eq!(
                    buf.cell((x, y)).map(|c| c.symbol()),
                    Some("█"),
                    "cell ({x},{y}) should be filled"
                );
            }
        }
    }

    #[test]
    fn test_fill_cell_render_zero_area() {
        let fill = FillCell::new('X', 10);
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        // Should not panic, and buf should remain untouched
        fill.render(area, &mut buf);
        // No cells to check — just verifying no crash
    }

    #[test]
    fn test_fill_cell_render_zero_width() {
        let fill = FillCell::new('X', 5);
        let area = Rect::new(0, 0, 0, 5);
        let mut buf = Buffer::empty(area);
        fill.render(area, &mut buf);
        // No crash — no cells at width 0
    }

    #[test]
    fn test_fill_cell_render_zero_height() {
        let fill = FillCell::new('X', 0);
        let area = Rect::new(0, 0, 80, 0);
        let mut buf = Buffer::empty(area);
        fill.render(area, &mut buf);
        // No crash — no cells at height 0
    }

    #[test]
    fn test_fill_cell_desired_height() {
        let fill = FillCell::new('A', 5);
        assert_eq!(fill.desired_height(80), 5);
        assert_eq!(fill.desired_height(0), 5);
        assert_eq!(fill.desired_height(u16::MAX), 5);
    }

    #[test]
    fn test_fill_cell_desired_height_zero() {
        let fill = FillCell::new('A', 0);
        assert_eq!(fill.desired_height(80), 0);
    }

    #[test]
    fn test_fill_cell_render_offset_area() {
        let fill = FillCell::new('Z', 2);
        let area = Rect::new(5, 3, 3, 2);
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 10));
        fill.render(area, &mut buf);

        // Cells inside the area should be set
        assert_eq!(buf.cell((5, 3)).map(|c| c.symbol()), Some("Z"));
        assert_eq!(buf.cell((7, 4)).map(|c| c.symbol()), Some("Z"));
        // Cells outside the area should remain spaces
        assert_eq!(buf.cell((4, 3)).map(|c| c.symbol()), Some(" "));
        assert_eq!(buf.cell((5, 2)).map(|c| c.symbol()), Some(" "));
    }

    // -----------------------------------------------------------------------
    // ColumnRenderable — boundary & edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_column_single_child() {
        let a = FillCell::new('A', 5);
        let column = ColumnRenderable::new(vec![&a]);
        assert_eq!(column.desired_height(80), 5);

        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        column.render(area, &mut buf);
        // A fills the first 5 rows
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("A"));
        assert_eq!(buf.cell((0, 4)).map(|c| c.symbol()), Some("A"));
        // Row 5+ should be unfilled (space)
        assert_eq!(buf.cell((0, 5)).map(|c| c.symbol()), Some(" "));
    }

    #[test]
    fn test_column_zero_width() {
        let a = FillCell::new('A', 10);
        let b = FillCell::new('B', 10);
        let column = ColumnRenderable::new(vec![&a, &b]);

        // At width 0, desired_height still returns sum (width doesn't affect FillCell)
        assert_eq!(column.desired_height(0), 20);

        // Render with zero-width area
        let area = Rect::new(0, 0, 0, 20);
        let mut buf = Buffer::empty(area);
        column.render(area, &mut buf);
        // No cells to write — just no crash
    }

    #[test]
    fn test_column_overflow_single_child_fills_remaining() {
        // When a single child's desired_height exceeds the area, it should be clipped
        let a = FillCell::new('A', 100);
        let column = ColumnRenderable::new(vec![&a]);

        let area = Rect::new(0, 0, 80, 3);
        let mut buf = Buffer::empty(area);
        column.render(area, &mut buf);

        // A fills all 3 lines
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("A"));
        assert_eq!(buf.cell((0, 2)).map(|c| c.symbol()), Some("A"));
    }

    #[test]
    fn test_column_overflow_multiple_children_all_clipped() {
        let a = FillCell::new('A', 10);
        let b = FillCell::new('B', 10);
        let column = ColumnRenderable::new(vec![&a, &b]);

        // Only 5 lines available — A gets 5, B gets 0
        let area = Rect::new(0, 0, 80, 5);
        let mut buf = Buffer::empty(area);
        column.render(area, &mut buf);

        for y in 0..5 {
            assert_eq!(
                buf.cell((0, y)).map(|c| c.symbol()),
                Some("A"),
                "row {y} should be A"
            );
        }
    }

    #[test]
    fn test_column_overflow_partial_second_child() {
        let a = FillCell::new('A', 3);
        let b = FillCell::new('B', 10);
        let column = ColumnRenderable::new(vec![&a, &b]);

        // 5 lines: A gets 3, B gets clipped to 2
        let area = Rect::new(0, 0, 80, 5);
        let mut buf = Buffer::empty(area);
        column.render(area, &mut buf);

        for y in 0..3 {
            assert_eq!(buf.cell((0, y)).map(|c| c.symbol()), Some("A"));
        }
        for y in 3..5 {
            assert_eq!(buf.cell((0, y)).map(|c| c.symbol()), Some("B"));
        }
    }

    #[test]
    fn test_column_empty_children_render_noop() {
        let column = ColumnRenderable::<'_>::new(vec![]);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        // Should not panic, and buf should remain all spaces
        column.render(area, &mut buf);
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some(" "));
    }

    #[test]
    fn test_column_desired_height_zero_width() {
        let a = FillCell::new('A', 5);
        let b = FillCell::new('B', 3);
        let column = ColumnRenderable::new(vec![&a, &b]);
        // FillCell ignores width, so sum is still 8
        assert_eq!(column.desired_height(0), 8);
    }

    #[test]
    fn test_column_render_with_offset_area() {
        let a = FillCell::new('A', 2);
        let b = FillCell::new('B', 2);
        let column = ColumnRenderable::new(vec![&a, &b]);

        // Area starts at (5, 5)
        let area = Rect::new(5, 5, 10, 4);
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 20));
        column.render(area, &mut buf);

        // A at rows 5-6, B at rows 7-8 (but area is only 4 tall, so B gets clipped to rows 7-8)
        // Actually: A gets 2 rows (5,6), B gets 2 rows (7,8) — fits in 4 rows
        assert_eq!(buf.cell((5, 5)).map(|c| c.symbol()), Some("A"));
        assert_eq!(buf.cell((5, 6)).map(|c| c.symbol()), Some("A"));
        assert_eq!(buf.cell((5, 7)).map(|c| c.symbol()), Some("B"));
        assert_eq!(buf.cell((5, 8)).map(|c| c.symbol()), Some("B"));
        // Outside the column area should remain space
        assert_eq!(buf.cell((5, 4)).map(|c| c.symbol()), Some(" "));
        assert_eq!(buf.cell((5, 9)).map(|c| c.symbol()), Some(" "));
    }

    // -----------------------------------------------------------------------
    // FlexRenderable — boundary & edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_flex_single_child() {
        let a = FillCell::new('A', 0);
        let flex = FlexRenderable::new(vec![(&a, 1)]);

        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        flex.render(area, &mut buf);

        // Single child with weight 1 takes all 10 rows
        for y in 0..10 {
            assert_eq!(buf.cell((0, y)).map(|c| c.symbol()), Some("A"));
        }
    }

    #[test]
    fn test_flex_rounding_truncation() {
        // 10 height / 3 weight = 3 per child with 1 remainder lost to truncation
        let a = FillCell::new('A', 0);
        let b = FillCell::new('B', 0);
        let c = FillCell::new('C', 0);
        let flex = FlexRenderable::new(vec![(&a, 1), (&b, 1), (&c, 1)]);

        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        flex.render(area, &mut buf);

        // Each gets 10 * 1 / 3 = 3 rows (truncated). Total = 9, 1 row lost.
        for y in 0..3 {
            assert_eq!(buf.cell((0, y)).map(|c| c.symbol()), Some("A"));
        }
        for y in 3..6 {
            assert_eq!(buf.cell((0, y)).map(|c| c.symbol()), Some("B"));
        }
        for y in 6..9 {
            assert_eq!(buf.cell((0, y)).map(|c| c.symbol()), Some("C"));
        }
        // Row 9 is untouched (space)
        assert_eq!(buf.cell((0, 9)).map(|c| c.symbol()), Some(" "));
    }

    #[test]
    fn test_flex_zero_area() {
        let a = FillCell::new('A', 0);
        let b = FillCell::new('B', 0);
        let flex = FlexRenderable::new(vec![(&a, 1), (&b, 1)]);

        let area = Rect::new(0, 0, 80, 0);
        let mut buf = Buffer::empty(area);
        flex.render(area, &mut buf);
        // No crash — area height is 0, every child gets 0
    }

    #[test]
    fn test_flex_all_children_zero_weight() {
        let a = FillCell::new('A', 0);
        let b = FillCell::new('B', 0);
        let flex = FlexRenderable::new(vec![(&a, 0), (&b, 0)]);

        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        flex.render(area, &mut buf);
        // total_weight == 0, early return — no crash, nothing rendered
        for y in 0..10 {
            assert_eq!(buf.cell((0, y)).map(|c| c.symbol()), Some(" "));
        }
    }

    #[test]
    fn test_flex_mixed_zero_and_nonzero_weights() {
        let a = FillCell::new('A', 0);
        let b = FillCell::new('B', 0);
        let c = FillCell::new('C', 0);
        let flex = FlexRenderable::new(vec![(&a, 0), (&b, 1), (&c, 3)]);

        let area = Rect::new(0, 0, 80, 8);
        let mut buf = Buffer::empty(area);
        flex.render(area, &mut buf);

        // total_weight = 4, child with weight 0 gets 0 rows
        for _y in 0..0 {
            // no rows for weight 0
        }
        // B gets 8 * 1 / 4 = 2 rows
        for y in 0..2 {
            assert_eq!(buf.cell((0, y)).map(|c| c.symbol()), Some("B"));
        }
        // C gets 8 * 3 / 4 = 6 rows
        for y in 2..8 {
            assert_eq!(buf.cell((0, y)).map(|c| c.symbol()), Some("C"));
        }
    }

    #[test]
    fn test_flex_unequal_weights() {
        let a = FillCell::new('A', 0);
        let b = FillCell::new('B', 0);
        let flex = FlexRenderable::new(vec![(&a, 7), (&b, 3)]);

        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        flex.render(area, &mut buf);

        // A gets 10 * 7 / 10 = 7 rows, B gets 10 * 3 / 10 = 3 rows
        for y in 0..7 {
            assert_eq!(buf.cell((0, y)).map(|c| c.symbol()), Some("A"));
        }
        for y in 7..10 {
            assert_eq!(buf.cell((0, y)).map(|c| c.symbol()), Some("B"));
        }
    }

    #[test]
    fn test_flex_desired_height_always_zero() {
        let a = FillCell::new('A', 0);
        let b = FillCell::new('B', 0);
        let flex = FlexRenderable::new(vec![(&a, 5), (&b, 5)]);
        assert_eq!(flex.desired_height(80), 0);
        assert_eq!(flex.desired_height(0), 0);
        assert_eq!(flex.desired_height(u16::MAX), 0);
    }

    #[test]
    fn test_flex_empty_children_no_crash() {
        let flex = FlexRenderable::<'_>::new(vec![]);
        assert!(flex.is_empty());
        assert_eq!(flex.len(), 0);
        assert_eq!(flex.desired_height(80), 0);

        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        flex.render(area, &mut buf);
        // No crash — nothing to render
    }

    #[test]
    fn test_flex_render_with_offset() {
        let a = FillCell::new('A', 0);
        let b = FillCell::new('B', 0);
        let flex = FlexRenderable::new(vec![(&a, 1), (&b, 1)]);

        let area = Rect::new(5, 5, 10, 4);
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 20));
        flex.render(area, &mut buf);

        // Each gets 2 rows at offset (5,5)
        for y in 5..7 {
            assert_eq!(buf.cell((5, y)).map(|c| c.symbol()), Some("A"));
        }
        for y in 7..9 {
            assert_eq!(buf.cell((5, y)).map(|c| c.symbol()), Some("B"));
        }
    }

    // -----------------------------------------------------------------------
    // InsetRenderable — boundary & edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_inset_zero_padding() {
        let inner = FillCell::new('X', 3);
        let inset = InsetRenderable::new(&inner); // all padding = 0

        assert_eq!(inset.desired_height(80), 3);
        assert_eq!(inset.top, 0);
        assert_eq!(inset.bottom, 0);
        assert_eq!(inset.left, 0);
        assert_eq!(inset.right, 0);

        let area = Rect::new(0, 0, 10, 3);
        let mut buf = Buffer::empty(area);
        inset.render(area, &mut buf);

        // Inner renders at the same position as the outer area
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("X"));
    }

    #[test]
    fn test_inset_padding_larger_than_area_width() {
        let inner = FillCell::new('X', 3);
        let inset = InsetRenderable::new(&inner)
            .with_left(10)
            .with_right(10);

        // Inner area width = 80.saturating_sub(20) = 60 still fine
        // But if area is small...
        let area = Rect::new(0, 0, 5, 5);
        let mut buf = Buffer::empty(area);
        inset.render(area, &mut buf);
        // inner area width = 5.saturating_sub(20) = 0, height = 5
        // Inner renders at x=10,y=0,w=0,h=5 — nothing visible, no crash
    }

    #[test]
    fn test_inset_padding_larger_than_area_height() {
        let inner = FillCell::new('X', 3);
        let inset = InsetRenderable::new(&inner)
            .with_top(10)
            .with_bottom(10);

        let area = Rect::new(0, 0, 80, 5);
        let mut buf = Buffer::empty(area);
        inset.render(area, &mut buf);
        // inner area height = 5.saturating_sub(20) = 0 — no crash
    }

    #[test]
    fn test_inset_only_left() {
        let inner = FillCell::new('X', 2);
        let inset = InsetRenderable::new(&inner).with_left(3);

        let area = Rect::new(0, 0, 10, 2);
        let mut buf = Buffer::empty(area);
        inset.render(area, &mut buf);

        // Inner area starts at x=3, y=0, width=7, height=2
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some(" "));
        assert_eq!(buf.cell((3, 0)).map(|c| c.symbol()), Some("X"));
        assert_eq!(buf.cell((3, 1)).map(|c| c.symbol()), Some("X"));
    }

    #[test]
    fn test_inset_only_right() {
        let inner = FillCell::new('X', 2);
        let inset = InsetRenderable::new(&inner).with_right(3);

        let area = Rect::new(0, 0, 10, 2);
        let mut buf = Buffer::empty(area);
        inset.render(area, &mut buf);

        // Inner area width = 10 - 3 = 7, starts at x=0
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("X"));
        assert_eq!(buf.cell((6, 0)).map(|c| c.symbol()), Some("X"));
        // Inside the padding area (right 3 cols) should be space
        assert_eq!(buf.cell((7, 0)).map(|c| c.symbol()), Some(" "));
    }

    #[test]
    fn test_inset_only_top() {
        let inner = FillCell::new('X', 2);
        let inset = InsetRenderable::new(&inner).with_top(2);

        let area = Rect::new(0, 0, 5, 4);
        let mut buf = Buffer::empty(area);
        inset.render(area, &mut buf);

        // Inner area starts at y=2, height=2, width=5
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some(" "));
        assert_eq!(buf.cell((0, 1)).map(|c| c.symbol()), Some(" "));
        assert_eq!(buf.cell((0, 2)).map(|c| c.symbol()), Some("X"));
        assert_eq!(buf.cell((0, 3)).map(|c| c.symbol()), Some("X"));
    }

    #[test]
    fn test_inset_only_bottom() {
        let inner = FillCell::new('X', 2);
        let inset = InsetRenderable::new(&inner).with_bottom(2);

        assert_eq!(inset.desired_height(80), 4); // 2 + 2
        // desired_height w/ padding: inner.desired_height(width - 0) + 0 + 2 = 2 + 2 = 4
    }

    #[test]
    fn test_inset_desired_height_zero_width_after_padding() {
        let inner = FillCell::new('X', 5);
        let inset = InsetRenderable::new(&inner)
            .with_left(10)
            .with_right(10);
        // inner.desired_height(80.saturating_sub(20) = 60) = 5, + 0 + 0 = 5
        assert_eq!(inset.desired_height(80), 5);
        // inner.desired_height(10.saturating_sub(20) = 0) = 5, + 0 + 0 = 5
        assert_eq!(inset.desired_height(10), 5);
    }

    #[test]
    fn test_inset_chaining_order() {
        let inner = FillCell::new('X', 2);
        let inset = InsetRenderable::new(&inner)
            .with_all(1)
            .with_top(3); // top overrides to 3

        let area = Rect::new(0, 0, 10, 6);
        let mut buf = Buffer::empty(area);
        inset.render(area, &mut buf);

        // Inner area starts at y=3 (top=3), x=1 (left=1)
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some(" "));
        assert_eq!(buf.cell((1, 3)).map(|c| c.symbol()), Some("X"));
    }

    // -----------------------------------------------------------------------
    // Renderable trait — custom cursor implementations
    // -----------------------------------------------------------------------

    #[test]
    fn test_trait_cursor_pos_some() {
        struct CursorInput {
            cursor_col: u16,
            cursor_row: u16,
        }
        impl Renderable for CursorInput {
            fn render(&self, _area: Rect, _buf: &mut Buffer) {}
            fn desired_height(&self, _width: u16) -> u16 {
                1
            }
            fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
                Some((area.x + self.cursor_col, area.y + self.cursor_row))
            }
        }

        let input = CursorInput {
            cursor_col: 5,
            cursor_row: 0,
        };
        let area = Rect::new(2, 3, 80, 1);
        assert_eq!(input.cursor_pos(area), Some((7, 3)));
    }

    #[test]
    fn test_trait_cursor_style_custom() {
        struct CustomCursor;
        impl Renderable for CustomCursor {
            fn render(&self, _area: Rect, _buf: &mut Buffer) {}
            fn desired_height(&self, _width: u16) -> u16 {
                0
            }
            fn cursor_style(&self, _area: Rect) -> SetCursorStyle {
                SetCursorStyle::BlinkingBar
            }
        }

        let c = CustomCursor;
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(c.cursor_style(area), SetCursorStyle::BlinkingBar);
    }

    #[test]
    fn test_trait_cursor_pos_none_by_default() {
        struct NoCursor;
        impl Renderable for NoCursor {
            fn render(&self, _area: Rect, _buf: &mut Buffer) {}
            fn desired_height(&self, _width: u16) -> u16 {
                0
            }
        }

        let nc = NoCursor;
        assert_eq!(nc.cursor_pos(Rect::new(0, 0, 80, 24)), None);
    }

    // -----------------------------------------------------------------------
    // Integration: composite layout components
    // -----------------------------------------------------------------------

    #[test]
    fn test_column_inside_inset() {
        let a = FillCell::new('A', 2);
        let b = FillCell::new('B', 2);
        let column = ColumnRenderable::new(vec![&a, &b]);
        let inset = InsetRenderable::new(&column).with_all(1);

        assert_eq!(inset.desired_height(80), 6); // 4 + 1 + 1

        let area = Rect::new(0, 0, 10, 6);
        let mut buf = Buffer::empty(area);
        inset.render(area, &mut buf);

        // Row 0 is top padding (space)
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some(" "));
        // Row 1-2 is A, col 1+ (col 0 is left padding)
        assert_eq!(buf.cell((1, 1)).map(|c| c.symbol()), Some("A"));
        assert_eq!(buf.cell((1, 2)).map(|c| c.symbol()), Some("A"));
        // Row 3-4 is B
        assert_eq!(buf.cell((1, 3)).map(|c| c.symbol()), Some("B"));
        assert_eq!(buf.cell((1, 4)).map(|c| c.symbol()), Some("B"));
        // Row 5 is bottom padding (space)
        assert_eq!(buf.cell((0, 5)).map(|c| c.symbol()), Some(" "));
    }

    #[test]
    fn test_flex_inside_inset() {
        let a = FillCell::new('A', 0);
        let b = FillCell::new('B', 0);
        let flex = FlexRenderable::new(vec![(&a, 1), (&b, 3)]);
        let inset = InsetRenderable::new(&flex).with_all(1);

        // flex.desired_height = 0, padding adds 2
        assert_eq!(inset.desired_height(80), 2);

        let area = Rect::new(0, 0, 10, 6);
        let mut buf = Buffer::empty(area);
        inset.render(area, &mut buf);
        // inner area = (1,1,8,4) — A gets 1 row, B gets 3 rows
        assert_eq!(buf.cell((1, 1)).map(|c| c.symbol()), Some("A"));
        assert_eq!(buf.cell((1, 2)).map(|c| c.symbol()), Some("B"));
        assert_eq!(buf.cell((1, 4)).map(|c| c.symbol()), Some("B"));
    }

    #[test]
    fn test_column_of_flex_children() {
        // Top section: flex with 2 children
        // Bottom section: a single FillCell
        let fa = FillCell::new('A', 0);
        let fb = FillCell::new('B', 0);
        let flex = FlexRenderable::new(vec![(&fa, 1), (&fb, 1)]);
        let bottom = FillCell::new('C', 1);

        let column = ColumnRenderable::new(vec![&flex as &dyn Renderable, &bottom]);

        // flex.desired_height = 0, bottom.desired_height = 1
        assert_eq!(column.desired_height(80), 1);

        let area = Rect::new(0, 0, 10, 5);
        let mut buf = Buffer::empty(area);
        column.render(area, &mut buf);

        // bottom gets 1 row at the top (row 0), flex gets area at rows 1-4
        // Actually: ColumnRenderable renders children in order.
        // Child 0 (flex): desired_height(10) = 0, so takes 0 rows.
        // Child 1 (bottom): desired_height(10) = 1, clipped to remaining 5 rows, takes 1 row at y=0.
        // So flex gets 0 rows (its desired_height is 0), bottom gets 1 row.
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("C"));
        // Rows 1-4 are untouched (the flex with 0 height gets nothing)
    }

    // -----------------------------------------------------------------------
    // FillCell — edge cases for non-ASCII characters
    // -----------------------------------------------------------------------

    #[test]
    fn test_fill_cell_with_unicode() {
        let fill = FillCell::new('🔥', 2);
        let area = Rect::new(0, 0, 2, 2);
        let mut buf = Buffer::empty(area);
        fill.render(area, &mut buf);

        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("🔥"));
        assert_eq!(buf.cell((1, 0)).map(|c| c.symbol()), Some("🔥"));
    }

    #[test]
    fn test_fill_cell_with_space() {
        let fill = FillCell::new(' ', 3);
        let area = Rect::new(0, 0, 5, 3);
        let mut buf = Buffer::empty(area);
        fill.render(area, &mut buf);

        // Space is already the default — still should be set
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some(" "));
    }

    // -----------------------------------------------------------------------
    // ColumnRenderable — collection mutability
    // -----------------------------------------------------------------------

    #[test]
    fn test_column_push_after_creation() {
        let a = FillCell::new('A', 1);
        let mut column = ColumnRenderable::new(vec![&a]);
        assert_eq!(column.len(), 1);

        let b = FillCell::new('B', 2);
        let c = FillCell::new('C', 3);
        column.push(&b);
        column.push(&c);
        assert_eq!(column.len(), 3);
        assert_eq!(column.desired_height(80), 6);
    }

    // -----------------------------------------------------------------------
    // FlexRenderable — collection mutability
    // -----------------------------------------------------------------------

    #[test]
    fn test_flex_push_after_creation() {
        let a = FillCell::new('A', 0);
        let mut flex = FlexRenderable::new(vec![(&a, 2)]);
        assert_eq!(flex.len(), 1);

        let b = FillCell::new('B', 0);
        flex.push(&b, 3);
        assert_eq!(flex.len(), 2);
        assert_eq!(flex.desired_height(80), 0); // flex always returns 0
    }

    // -----------------------------------------------------------------------
    // InsetRenderable — builder pattern returns Self
    // -----------------------------------------------------------------------

    #[test]
    fn test_inset_builder_returns_self() {
        let inner = FillCell::new('X', 1);
        let inset = InsetRenderable::new(&inner)
            .with_top(1)
            .with_bottom(2)
            .with_left(3)
            .with_right(4);

        assert_eq!(inset.top, 1);
        assert_eq!(inset.bottom, 2);
        assert_eq!(inset.left, 3);
        assert_eq!(inset.right, 4);
    }
}