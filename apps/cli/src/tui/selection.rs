//! Selection list — a filterable picker for choosing from a list of items.
//!
//! # Overview
//!
//! - [`SelectionItem`] — a single selectable item with label, description,
//!   and value.
//! - [`SelectionList`] — a [`PaneView`] that renders a scrollable, filterable
//!   list. Supports keyboard navigation, incremental search filtering, and
//!   cancel/confirm actions.
//!
//! # Key bindings
//!
//! | Key | Action |
//! |-----|--------|
//! | ↑ / k | Move selection up |
//! | ↓ / j | Move selection down |
//! | Enter | Confirm the selected item |
//! | Esc   | Cancel (no selection) |
//! | (any char) | Append to filter string |
//! | Backspace | Remove last character from filter |

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Widget},
};

use super::pane::{CtrlCAction, Handled, PaneView};
use super::render::Renderable;

// ---------------------------------------------------------------------------
// SelectionItem
// ---------------------------------------------------------------------------

/// A single item in a [`SelectionList`].
///
/// Each item has a human-readable `label`, an optional `description` shown
/// alongside the label, and a `value` returned when the item is selected.
#[derive(Debug, Clone)]
pub struct SelectionItem {
    /// Display label shown in the list.
    pub label: String,
    /// Optional description shown next to the label.
    pub description: Option<String>,
    /// Value returned when this item is selected (e.g. a model name, option).
    pub value: String,
}

impl SelectionItem {
    /// Create a new selection item with just a label and value.
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: None,
            value: value.into(),
        }
    }

    /// Create a new selection item with a label, description, and value.
    pub fn with_description(
        label: impl Into<String>,
        description: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            description: Some(description.into()),
            value: value.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// SelectionList
// ---------------------------------------------------------------------------

/// A filterable, scrollable selection list that implements [`PaneView`].
///
/// The list can be filtered by typing characters; the filter is matched
/// case-insensitively against item labels. Pressing Enter confirms the
/// currently highlighted item, Esc cancels without a selection.
///
/// # Example
///
/// ```ignore
/// let items = vec![
///     SelectionItem::new("Option A", "a"),
///     SelectionItem::new("Option B", "b"),
/// ];
/// let mut list = SelectionList::new(items, "选择模型".to_string());
/// ```
pub struct SelectionList {
    /// All items in the list (unfiltered).
    items: Vec<SelectionItem>,
    /// Index of the currently selected item within the *visible* subset.
    selected: usize,
    /// Current filter text (typed characters).
    filter: String,
    /// Whether the user has confirmed or cancelled (pane is done).
    confirmed: bool,
    /// The selected value, if confirmed by pressing Enter.
    result: Option<String>,
    /// Title displayed in the list's border.
    title: String,
}

impl SelectionList {
    /// Create a new selection list with the given items and title.
    pub fn new(items: Vec<SelectionItem>, title: String) -> Self {
        Self {
            items,
            selected: 0,
            filter: String::new(),
            confirmed: false,
            result: None,
            title,
        }
    }

    /// Return the selected value, if the user confirmed with Enter.
    ///
    /// Returns `None` if the user cancelled (Esc) or hasn't chosen yet.
    pub fn result(&self) -> Option<&str> {
        self.result.as_deref()
    }

    /// All items that match the current filter.
    ///
    /// When the filter is empty, every item is visible. Filtering is
    /// case-insensitive and matches against the item's label.
    pub fn visible_items(&self) -> Vec<&SelectionItem> {
        if self.filter.is_empty() {
            self.items.iter().collect()
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.items
                .iter()
                .filter(|item| item.label.to_lowercase().contains(&filter_lower))
                .collect()
        }
    }
}

// ---------------------------------------------------------------------------
// PaneView impl
// ---------------------------------------------------------------------------

impl PaneView for SelectionList {
    fn handle_key_event(&mut self, key: KeyEvent) -> Handled {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                Handled::Handled
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let visible = self.visible_items();
                if self.selected + 1 < visible.len() {
                    self.selected += 1;
                }
                Handled::Handled
            }
            KeyCode::Enter => {
                let visible = self.visible_items();
                if let Some(item) = visible.get(self.selected) {
                    self.result = Some(item.value.clone());
                    self.confirmed = true;
                }
                Handled::Handled
            }
            KeyCode::Esc => {
                // Cancel: mark as complete without a result.
                self.confirmed = true;
                Handled::Handled
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.selected = 0;
                Handled::Handled
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.selected = 0;
                Handled::Handled
            }
            _ => Handled::NotHandled,
        }
    }

    fn is_complete(&self) -> bool {
        self.confirmed
    }

    fn on_ctrl_c(&mut self) -> CtrlCAction {
        self.confirmed = true;
        CtrlCAction::Handled
    }

    fn view_id(&self) -> Option<&'static str> {
        Some("selection")
    }
}

// ---------------------------------------------------------------------------
// Renderable impl
// ---------------------------------------------------------------------------

impl Renderable for SelectionList {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        block.render(area, buf);

        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );

        let visible = self.visible_items();
        let max_items = inner.height as usize;
        let start = self
            .selected
            .saturating_sub(max_items.saturating_sub(1));

        for (i, item) in visible.iter().enumerate().skip(start) {
            let y = inner.y + (i - start) as u16;
            if y >= inner.y + inner.height {
                break;
            }

            let is_selected = i == self.selected;
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };

            let label = if let Some(desc) = &item.description {
                format!(" {} - {}", item.label, desc)
            } else {
                format!(" {}", item.label)
            };

            let span = Span::styled(label, style);
            span.render(Rect::new(inner.x, y, inner.width, 1), buf);
        }

        // Show filter hint at the bottom when filtering is active.
        if !self.filter.is_empty() {
            let filter_text = format!(" 过滤: {}", self.filter);
            let filter_span = Span::styled(
                filter_text,
                Style::default().fg(Color::Yellow),
            );
            let filter_area = Rect::new(
                inner.x,
                inner.y + inner.height.saturating_sub(1),
                inner.width,
                1,
            );
            filter_span.render(filter_area, buf);
        }
    }

    fn desired_height(&self, _width: u16) -> u16 {
        let visible_count = self.visible_items().len() as u16;
        // +2 for the border; cap at 15 rows so the list doesn't overflow.
        (visible_count + 2).min(15)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::cursor::SetCursorStyle;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers,
    };
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    use super::super::pane::{CtrlCAction, Handled};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Convenience constructor for a press key event with no modifiers.
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// Build a list of N items with labels "Item 0", "Item 1", ..., "Item N-1".
    fn make_items(n: usize) -> Vec<SelectionItem> {
        (0..n)
            .map(|i| SelectionItem::new(format!("Item {i}"), format!("value_{i}")))
            .collect()
    }

    // -----------------------------------------------------------------------
    // SelectionItem construction
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_item_new() {
        let item = SelectionItem::new("Label", "val");
        assert_eq!(item.label, "Label");
        assert_eq!(item.value, "val");
        assert!(item.description.is_none());
    }

    #[test]
    fn test_selection_item_new_into_string() {
        let item = SelectionItem::new(String::from("Foo"), String::from("bar"));
        assert_eq!(item.label, "Foo");
        assert_eq!(item.value, "bar");
    }

    #[test]
    fn test_selection_item_with_description() {
        let item =
            SelectionItem::with_description("Label", "A description", "val");
        assert_eq!(item.label, "Label");
        assert_eq!(item.description.as_deref(), Some("A description"));
        assert_eq!(item.value, "val");
    }

    #[test]
    fn test_selection_item_clone() {
        let a = SelectionItem::with_description("L", "D", "v");
        let b = a.clone();
        assert_eq!(a.label, b.label);
        assert_eq!(a.description, b.description);
        assert_eq!(a.value, b.value);
    }

    #[test]
    fn test_selection_item_debug() {
        let item = SelectionItem::new("X", "y");
        let debug = format!("{item:?}");
        assert!(debug.contains("X"));
        assert!(debug.contains("y"));
    }

    // -----------------------------------------------------------------------
    // SelectionList — initial state
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_new_empty() {
        let list = SelectionList::new(vec![], "Title".to_string());
        assert!(list.visible_items().is_empty());
        assert!(list.result().is_none());
        // PaneView defaults
        assert!(!list.is_complete());
        assert_eq!(list.view_id(), Some("selection"));
    }

    #[test]
    fn test_selection_list_new_with_items() {
        let items = make_items(3);
        let list = SelectionList::new(items, "Pick".to_string());
        assert_eq!(list.visible_items().len(), 3);
        assert_eq!(list.visible_items()[0].label, "Item 0");
    }

    #[test]
    fn test_selection_list_result_none_before_confirm() {
        let mut list = SelectionList::new(make_items(2), "T".to_string());
        assert!(list.result().is_none());
        // After pressing an unrelated key, still no result
        list.handle_key_event(key(KeyCode::Char('x')));
        assert!(list.result().is_none());
    }

    // -----------------------------------------------------------------------
    // SelectionList — visible_items / filtering
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_visible_items_empty_filter_shows_all() {
        let items = make_items(5);
        let list = SelectionList::new(items, "T".to_string());
        assert_eq!(list.visible_items().len(), 5);
    }

    #[test]
    fn test_selection_list_visible_items_filter_matches() {
        let mut items = make_items(5);
        items.push(SelectionItem::new("Apple", "apple"));
        items.push(SelectionItem::new("Banana", "banana"));
        items.push(SelectionItem::new("Cherry", "cherry"));
        let mut list = SelectionList::new(items, "T".to_string());

        // Type 'a' — should match Apple and Banana (case-insensitive)
        list.handle_key_event(key(KeyCode::Char('a')));
        let visible = list.visible_items();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].label, "Apple");
        assert_eq!(visible[1].label, "Banana");
    }

    #[test]
    fn test_selection_list_visible_items_filter_case_insensitive() {
        let mut items = make_items(3);
        items.push(SelectionItem::new("APPLE", "a"));
        items.push(SelectionItem::new("apple", "b"));
        let mut list = SelectionList::new(items, "T".to_string());

        list.handle_key_event(key(KeyCode::Char('A')));
        assert_eq!(list.visible_items().len(), 2);
    }

    #[test]
    fn test_selection_list_visible_items_no_match() {
        let items = vec![
            SelectionItem::new("Cat", "c"),
            SelectionItem::new("Dog", "d"),
        ];
        let mut list = SelectionList::new(items, "T".to_string());

        list.handle_key_event(key(KeyCode::Char('z')));
        assert!(list.visible_items().is_empty());
    }

    #[test]
    fn test_selection_list_filter_backspace() {
        let mut items = make_items(3);
        items.push(SelectionItem::new("Apple", "a"));
        let mut list = SelectionList::new(items, "T".to_string());

        list.handle_key_event(key(KeyCode::Char('a')));
        assert_eq!(list.visible_items().len(), 1);

        list.handle_key_event(key(KeyCode::Backspace));
        assert_eq!(list.visible_items().len(), 4); // all items back
    }

    #[test]
    fn test_selection_list_filter_backspace_on_empty() {
        // Backspace on empty filter should not crash
        let mut list = SelectionList::new(make_items(2), "T".to_string());
        list.handle_key_event(key(KeyCode::Backspace));
        assert_eq!(list.visible_items().len(), 2);
    }

    // -----------------------------------------------------------------------
    // SelectionList — navigation
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_arrow_up() {
        let mut list = SelectionList::new(make_items(3), "T".to_string());
        // Move down first, then up
        assert_eq!(
            list.handle_key_event(key(KeyCode::Down)),
            Handled::Handled
        );
        assert_eq!(
            list.handle_key_event(key(KeyCode::Up)),
            Handled::Handled
        );
        // Enter should still select item 0
        assert_eq!(
            list.handle_key_event(key(KeyCode::Enter)),
            Handled::Handled
        );
        assert_eq!(list.result(), Some("value_0"));
    }

    #[test]
    fn test_selection_list_arrow_up_at_top() {
        let mut list = SelectionList::new(make_items(3), "T".to_string());
        // Up at top should stay at 0
        for _ in 0..5 {
            list.handle_key_event(key(KeyCode::Up));
        }
        list.handle_key_event(key(KeyCode::Enter));
        assert_eq!(list.result(), Some("value_0"));
    }

    #[test]
    fn test_selection_list_arrow_down() {
        let mut list = SelectionList::new(make_items(3), "T".to_string());
        list.handle_key_event(key(KeyCode::Down));
        list.handle_key_event(key(KeyCode::Enter));
        assert_eq!(list.result(), Some("value_1"));
    }

    #[test]
    fn test_selection_list_arrow_down_at_bottom() {
        let mut list = SelectionList::new(make_items(3), "T".to_string());
        // Move down past the end
        for _ in 0..10 {
            list.handle_key_event(key(KeyCode::Down));
        }
        list.handle_key_event(key(KeyCode::Enter));
        // Should be stuck at the last item
        assert_eq!(list.result(), Some("value_2"));
    }

    #[test]
    fn test_selection_list_arrow_down_on_empty_list() {
        let mut list = SelectionList::new(vec![], "T".to_string());
        // Down on empty list should not crash
        assert_eq!(
            list.handle_key_event(key(KeyCode::Down)),
            Handled::Handled
        );
        assert_eq!(
            list.handle_key_event(key(KeyCode::Up)),
            Handled::Handled
        );
    }

    #[test]
    fn test_selection_list_key_navigation_variants() {
        let mut list = SelectionList::new(make_items(3), "T".to_string());
        // 'j' = down
        list.handle_key_event(key(KeyCode::Char('j')));
        list.handle_key_event(key(KeyCode::Enter));
        assert_eq!(list.result(), Some("value_1"));

        // Reset and try 'k' = up
        let mut list2 = SelectionList::new(make_items(3), "T".to_string());
        list2.handle_key_event(key(KeyCode::Down));
        list2.handle_key_event(key(KeyCode::Char('k'))); // up
        list2.handle_key_event(key(KeyCode::Enter));
        assert_eq!(list2.result(), Some("value_0"));
    }

    // -----------------------------------------------------------------------
    // SelectionList — confirm / cancel
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_enter_confirms() {
        let mut list = SelectionList::new(make_items(3), "T".to_string());
        assert_eq!(
            list.handle_key_event(key(KeyCode::Enter)),
            Handled::Handled
        );
        assert!(list.is_complete());
        assert_eq!(list.result(), Some("value_0"));
    }

    #[test]
    fn test_selection_list_enter_on_empty_list() {
        let mut list = SelectionList::new(vec![], "T".to_string());
        // Enter on empty list should not crash, and should not set result
        // or mark as complete (no item to select).
        assert_eq!(
            list.handle_key_event(key(KeyCode::Enter)),
            Handled::Handled
        );
        assert!(!list.is_complete());
        assert!(list.result().is_none());
    }

    #[test]
    fn test_selection_list_enter_after_filter_selects_correct_item() {
        let mut items = make_items(5);
        items.push(SelectionItem::new("Apple", "apple_val"));
        items.push(SelectionItem::new("Orange", "orange_val"));
        let mut list = SelectionList::new(items, "T".to_string());

        // Filter for "apple"
        list.handle_key_event(key(KeyCode::Char('A')));
        list.handle_key_event(key(KeyCode::Enter));
        assert_eq!(list.result(), Some("apple_val"));
    }

    #[test]
    fn test_selection_list_esc_cancels() {
        let mut list = SelectionList::new(make_items(3), "T".to_string());
        assert_eq!(
            list.handle_key_event(key(KeyCode::Esc)),
            Handled::Handled
        );
        assert!(list.is_complete());
        assert!(list.result().is_none());
    }

    #[test]
    fn test_selection_list_is_complete_false_initially() {
        let list = SelectionList::new(make_items(2), "T".to_string());
        assert!(!list.is_complete());
    }

    #[test]
    fn test_selection_list_is_complete_true_after_enter() {
        let mut list = SelectionList::new(make_items(2), "T".to_string());
        list.handle_key_event(key(KeyCode::Enter));
        assert!(list.is_complete());
    }

    #[test]
    fn test_selection_list_is_complete_true_after_esc() {
        let mut list = SelectionList::new(make_items(2), "T".to_string());
        list.handle_key_event(key(KeyCode::Esc));
        assert!(list.is_complete());
    }

    // -----------------------------------------------------------------------
    // SelectionList — filter behavior
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_filter_appends_char() {
        let mut list = SelectionList::new(make_items(3), "T".to_string());
        list.handle_key_event(key(KeyCode::Char('x')));
        // visible_items only considers items whose label contains 'x';
        // since none do, the visible list is empty.
        assert!(list.visible_items().is_empty());
    }

    #[test]
    fn test_selection_list_filter_resets_selection_to_zero() {
        let mut items = make_items(5);
        // Add items with distinct first letters
        items.push(SelectionItem::new("Alpha", "a"));
        items.push(SelectionItem::new("Beta", "b"));
        items.push(SelectionItem::new("Gamma", "g"));
        let mut list = SelectionList::new(items, "T".to_string());

        // Move down to item 2
        list.handle_key_event(key(KeyCode::Down));
        list.handle_key_event(key(KeyCode::Down));

        // Type a character — selection resets to 0
        list.handle_key_event(key(KeyCode::Char('B')));
        // Enter should select the first visible item (Beta)
        list.handle_key_event(key(KeyCode::Enter));
        // The visible items are filtered to only "Beta"
        // So selected=0 means Beta
        assert_eq!(list.result(), Some("b"));
    }

    #[test]
    fn test_selection_list_backspace_resets_selection_to_zero() {
        let mut items = make_items(5);
        items.push(SelectionItem::new("Alpha", "a"));
        items.push(SelectionItem::new("Beta", "b"));
        let mut list = SelectionList::new(items, "T".to_string());

        // Filter, then backspace to clear filter
        list.handle_key_event(key(KeyCode::Char('B')));
        assert_eq!(list.visible_items().len(), 1);

        list.handle_key_event(key(KeyCode::Backspace));
        // Selection should be reset to 0; all items visible again
        assert_eq!(list.visible_items().len(), 7);
        list.handle_key_event(key(KeyCode::Enter));
        assert_eq!(list.result(), Some("value_0"));
    }

    // -----------------------------------------------------------------------
    // SelectionList — unhandled keys
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_unhandled_keys() {
        let mut list = SelectionList::new(make_items(2), "T".to_string());
        assert_eq!(
            list.handle_key_event(key(KeyCode::F(1))),
            Handled::NotHandled
        );
        assert_eq!(
            list.handle_key_event(key(KeyCode::Tab)),
            Handled::NotHandled
        );
        assert_eq!(
            list.handle_key_event(key(KeyCode::Home)),
            Handled::NotHandled
        );
        assert_eq!(
            list.handle_key_event(key(KeyCode::End)),
            Handled::NotHandled
        );
        assert_eq!(
            list.handle_key_event(key(KeyCode::Insert)),
            Handled::NotHandled
        );
        // State should be unchanged
        assert!(!list.is_complete());
        assert!(list.result().is_none());
    }

    // -----------------------------------------------------------------------
    // SelectionList — Ctrl+C behavior
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_on_ctrl_c() {
        let mut list = SelectionList::new(make_items(3), "T".to_string());
        assert_eq!(list.on_ctrl_c(), CtrlCAction::Handled);
        assert!(list.is_complete());
        assert!(list.result().is_none());
    }

    // -----------------------------------------------------------------------
    // SelectionList — view_id
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_view_id() {
        let list = SelectionList::new(make_items(1), "T".to_string());
        assert_eq!(list.view_id(), Some("selection"));
    }

    // -----------------------------------------------------------------------
    // Renderable: desired_height
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_desired_height_empty() {
        let list = SelectionList::new(vec![], "T".to_string());
        // 0 visible items + 2 border = 2
        assert_eq!(list.desired_height(80), 2);
    }

    #[test]
    fn test_selection_list_desired_height_with_items() {
        let list = SelectionList::new(make_items(5), "T".to_string());
        // 5 visible items + 2 border = 7
        assert_eq!(list.desired_height(80), 7);
    }

    #[test]
    fn test_selection_list_desired_height_capped_at_15() {
        let list = SelectionList::new(make_items(100), "T".to_string());
        // 100 visible items + 2 border = 102, capped at 15
        assert_eq!(list.desired_height(80), 15);
    }

    #[test]
    fn test_selection_list_desired_height_with_filter() {
        let mut items = make_items(10);
        items.push(SelectionItem::new("Apple", "a"));
        let mut list = SelectionList::new(items, "T".to_string());

        // After filtering to just "Apple", desired_height = 1 + 2 = 3
        list.handle_key_event(key(KeyCode::Char('A')));
        assert_eq!(list.visible_items().len(), 1);
        assert_eq!(list.desired_height(80), 3);
    }

    // -----------------------------------------------------------------------
    // Renderable: cursor_pos / cursor_style (defaults)
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_cursor_pos_none() {
        let list = SelectionList::new(make_items(3), "T".to_string());
        let area = Rect::new(0, 0, 80, 10);
        assert_eq!(list.cursor_pos(area), None);
    }

    #[test]
    fn test_selection_list_cursor_style_default() {
        let list = SelectionList::new(make_items(3), "T".to_string());
        let area = Rect::new(0, 0, 80, 10);
        assert_eq!(
            list.cursor_style(area),
            SetCursorStyle::DefaultUserShape
        );
    }

    // -----------------------------------------------------------------------
    // Renderable: render (smoke tests — no crash, basic content checks)
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_render_empty_area() {
        let list = SelectionList::new(make_items(3), "Test".to_string());
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        // Should not panic
        list.render(area, &mut buf);
    }

    #[test]
    fn test_selection_list_render_normal_area() {
        let list = SelectionList::new(make_items(3), "Test".to_string());
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        list.render(area, &mut buf);

        // Border should be rendered (corners of the outer rect)
        // Top-left corner of block
        if let Some(cell) = buf.cell((0, 0)) {
            // Border symbols are non-space
            assert_ne!(cell.symbol(), " ");
        }
    }

    #[test]
    fn test_selection_list_render_with_filter_shown() {
        let mut list = SelectionList::new(make_items(5), "Test".to_string());
        list.handle_key_event(key(KeyCode::Char('x')));

        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        list.render(area, &mut buf);

        // Filter hint should be rendered somewhere in the buffer
        // (we can't easily inspect the exact cell, but render should not panic)
    }

    #[test]
    fn test_selection_list_render_no_filter_hint_when_empty() {
        let list = SelectionList::new(make_items(5), "Test".to_string());
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        // Should not panic — no filter hint is shown
        list.render(area, &mut buf);
    }

    #[test]
    fn test_selection_list_render_with_description() {
        let items = vec![
            SelectionItem::with_description("Label", "Desc", "val"),
        ];
        let list = SelectionList::new(items, "Test".to_string());
        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        // Should not panic when rendering items with descriptions
        list.render(area, &mut buf);
    }

    #[test]
    fn test_selection_list_render_scroll_with_many_items() {
        let list = SelectionList::new(make_items(50), "Test".to_string());
        // Small area — should scroll without panic
        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        list.render(area, &mut buf);
    }

    // -----------------------------------------------------------------------
    // Renderable trait — method signature verification
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_implements_renderable() {
        // Compile-time check: SelectionList must satisfy Renderable
        fn assert_renderable<T: Renderable>(_: &T) {}
        let list = SelectionList::new(make_items(1), "T".to_string());
        assert_renderable(&list);
    }

    #[test]
    fn test_selection_list_implements_pane_view() {
        // Compile-time check: SelectionList must satisfy PaneView
        fn assert_pane_view<T: PaneView>(_: &T) {}
        let list = SelectionList::new(make_items(1), "T".to_string());
        assert_pane_view(&list);
    }

    // -----------------------------------------------------------------------
    // Integration: multiple operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_full_workflow_filter_navigate_confirm() {
        let mut items = make_items(10);
        items.push(SelectionItem::new("Alpha", "alpha_val"));
        items.push(SelectionItem::new("Beta", "beta_val"));
        items.push(SelectionItem::new("Gamma", "gamma_val"));
        let mut list = SelectionList::new(items, "Pick".to_string());

        // Filter for "a" — matches Alpha, Beta, Gamma (all contain 'a')
        list.handle_key_event(key(KeyCode::Char('a')));
        assert_eq!(list.visible_items().len(), 3);
        assert_eq!(list.visible_items()[0].label, "Alpha");

        // Confirm — selects first visible item (Alpha)
        list.handle_key_event(key(KeyCode::Enter));
        assert_eq!(list.result(), Some("alpha_val"));
        assert!(list.is_complete());
    }

    #[test]
    fn test_selection_list_filter_navigate_down_then_enter() {
        let mut items = make_items(3);
        items.push(SelectionItem::new("Apple", "a"));
        items.push(SelectionItem::new("Apricot", "ap"));
        items.push(SelectionItem::new("Avocado", "av"));
        let mut list = SelectionList::new(items, "T".to_string());

        // Filter for "ap"
        list.handle_key_event(key(KeyCode::Char('a')));
        list.handle_key_event(key(KeyCode::Char('p')));
        assert_eq!(list.visible_items().len(), 2); // Apple, Apricot

        // Move down
        list.handle_key_event(key(KeyCode::Down));
        list.handle_key_event(key(KeyCode::Enter));
        // Second item: Apricot
        assert_eq!(list.result(), Some("ap"));
    }

    #[test]
    fn test_selection_list_esc_after_filter() {
        let mut list = SelectionList::new(make_items(5), "T".to_string());
        list.handle_key_event(key(KeyCode::Char('x')));
        list.handle_key_event(key(KeyCode::Esc));
        assert!(list.is_complete());
        assert!(list.result().is_none());
    }

    #[test]
    fn test_selection_list_enter_on_filtered_empty_list() {
        let items = vec![
            SelectionItem::new("Cat", "c"),
            SelectionItem::new("Dog", "d"),
        ];
        let mut list = SelectionList::new(items, "T".to_string());

        // Filter to nothing
        list.handle_key_event(key(KeyCode::Char('z')));
        assert!(list.visible_items().is_empty());

        // Enter on empty filtered list should not crash
        // (no confirmation since no item is selectable)
        assert_eq!(
            list.handle_key_event(key(KeyCode::Enter)),
            Handled::Handled
        );
        assert!(!list.is_complete());
        assert!(list.result().is_none());
    }

    // -----------------------------------------------------------------------
    // Edge cases: single item
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_single_item() {
        let mut list = SelectionList::new(
            vec![SelectionItem::new("Only", "only_val")],
            "T".to_string(),
        );
        assert_eq!(list.visible_items().len(), 1);
        assert_eq!(list.desired_height(80), 3); // 1 + 2

        list.handle_key_event(key(KeyCode::Down)); // stays at 0
        list.handle_key_event(key(KeyCode::Up)); // stays at 0
        list.handle_key_event(key(KeyCode::Enter));
        assert_eq!(list.result(), Some("only_val"));
    }

    // -----------------------------------------------------------------------
    // Edge cases: large data
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_many_items_desired_height_capped() {
        let list = SelectionList::new(make_items(1000), "T".to_string());
        // Capped at 15 regardless of how many items
        assert_eq!(list.desired_height(80), 15);
    }

    #[test]
    fn test_selection_list_many_items_render_no_crash() {
        let list = SelectionList::new(make_items(1000), "T".to_string());
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        list.render(area, &mut buf);
    }

    // -----------------------------------------------------------------------
    // Edge cases: zero-width area
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_list_render_zero_width() {
        let list = SelectionList::new(make_items(5), "T".to_string());
        let area = Rect::new(0, 0, 0, 10);
        let mut buf = Buffer::empty(area);
        // Should not panic — saturating_sub handles width=0
        list.render(area, &mut buf);
    }

    #[test]
    fn test_selection_list_desired_height_zero_width() {
        let list = SelectionList::new(make_items(5), "T".to_string());
        // desired_height doesn't use width, so this should be fine
        assert_eq!(list.desired_height(0), 7);
    }
}