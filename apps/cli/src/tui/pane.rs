//! Stack-based panel manager and the PaneView trait.
//!
//! # Overview
//!
//! - [`PaneView`] — trait that every interactive panel implements.
//! - [`PaneStack`] — a stack of panels with an optional base panel.
//!
//! The stack is LIFO: the top-most panel receives input first. Completed
//! panels are automatically popped. The base panel is always present and
//! acts as the fallback when the stack is empty.

use crossterm::cursor::SetCursorStyle;
use crossterm::event::KeyEvent;

use super::render::Renderable;

// ---------------------------------------------------------------------------
// Handled
// ---------------------------------------------------------------------------

/// Whether a key event was consumed by a panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handled {
    /// Event was consumed; do not propagate further.
    Handled,
    /// Event was not consumed; may be passed to the next handler.
    NotHandled,
}

// ---------------------------------------------------------------------------
// CtrlCAction
// ---------------------------------------------------------------------------

/// How a panel responds to Ctrl+C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtrlCAction {
    /// Not handled; propagate to the next handler.
    NotHandled,
    /// Handled without cancelling; the application continues.
    Handled,
    /// The panel wants to cancel the current operation (e.g. close itself).
    Cancel,
}

// ---------------------------------------------------------------------------
// PaneView
// ---------------------------------------------------------------------------

/// A single interactive panel that can be pushed onto a [`PaneStack`].
///
/// Every pane is also [`Renderable`], so it can be drawn to the terminal.
pub trait PaneView: Renderable {
    /// Handle a key event. Return [`Handled::Handled`] when the event is
    /// consumed, [`Handled::NotHandled`] otherwise.
    fn handle_key_event(&mut self, key: KeyEvent) -> Handled;

    /// Whether this pane has completed its work and should be popped.
    fn is_complete(&self) -> bool {
        false
    }

    /// Handle Ctrl+C. The default implementation does nothing.
    fn on_ctrl_c(&mut self) -> CtrlCAction {
        CtrlCAction::NotHandled
    }

    /// Optional stable identifier for this pane type.
    fn view_id(&self) -> Option<&'static str> {
        None
    }
}

// ---------------------------------------------------------------------------
// PaneStack
// ---------------------------------------------------------------------------

/// A stack of panels with an optional base panel.
///
/// The **base** panel sits at the bottom and is always present (e.g. a chat
/// composer). The **stack** holds transient overlay panels (approval dialogs,
/// selection lists, etc.). Input is routed to the top-most panel first.
/// Completed stack panels are automatically popped.
pub struct PaneStack {
    /// Transient overlay panels, LIFO order.
    stack: Vec<Box<dyn PaneView>>,
    /// The persistent base panel (e.g. ChatComposer).
    base: Option<Box<dyn PaneView>>,
}

impl PaneStack {
    /// Create an empty pane stack.
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            base: None,
        }
    }

    /// Set (or replace) the base panel.
    pub fn set_base(&mut self, base: Box<dyn PaneView>) {
        self.base = Some(base);
    }

    /// Push a new panel onto the stack (becomes the active panel).
    pub fn push(&mut self, pane: Box<dyn PaneView>) {
        self.stack.push(pane);
    }

    /// Pop the top-most panel from the stack.
    pub fn pop(&mut self) -> Option<Box<dyn PaneView>> {
        self.stack.pop()
    }

    /// Return a mutable reference to the currently active panel.
    ///
    /// The stack is checked first; if empty the base panel is returned.
    fn active(&mut self) -> Option<&mut Box<dyn PaneView>> {
        match self.stack.last_mut() {
            Some(top) => Some(top),
            None => self.base.as_mut(),
        }
    }

    /// Route a key event to the active panel.
    ///
    /// If the active panel consumes the event, completed panels are cleaned
    /// up automatically.
    pub fn handle_key_event(&mut self, key: KeyEvent) -> Handled {
        if let Some(active) = self.active() {
            if active.handle_key_event(key) == Handled::Handled {
                self.cleanup_completed();
                return Handled::Handled;
            }
        }
        Handled::NotHandled
    }

    /// Route Ctrl+C to the active panel.
    ///
    /// Returns the panel's response, or `CtrlCAction::NotHandled` if no
    /// panel is active. The caller should only perform the global interrupt
    /// flow when the result is `NotHandled`.
    pub fn handle_ctrl_c(&mut self) -> CtrlCAction {
        if let Some(active) = self.active() {
            active.on_ctrl_c()
        } else {
            CtrlCAction::NotHandled
        }
    }

    /// Remove all completed panels from the top of the stack.
    fn cleanup_completed(&mut self) {
        while let Some(top) = self.stack.last() {
            if top.is_complete() {
                self.stack.pop();
            } else {
                break;
            }
        }
    }

    /// Number of panels currently on the stack (excluding the base).
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Whether any panel is active (stack non-empty or base is set).
    pub fn is_active(&self) -> bool {
        !self.stack.is_empty() || self.base.is_some()
    }
}

impl Default for PaneStack {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Renderable impl for PaneStack
// ---------------------------------------------------------------------------

impl Renderable for PaneStack {
    fn render(&self, area: ratatui::layout::Rect, buf: &mut ratatui::buffer::Buffer) {
        if let Some(top) = self.stack.last() {
            top.render(area, buf);
        } else if let Some(base) = &self.base {
            base.render(area, buf);
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        if let Some(top) = self.stack.last() {
            top.desired_height(width)
        } else if let Some(base) = &self.base {
            base.desired_height(width)
        } else {
            0
        }
    }

    fn cursor_pos(&self, area: ratatui::layout::Rect) -> Option<(u16, u16)> {
        if let Some(top) = self.stack.last() {
            top.cursor_pos(area)
        } else if let Some(base) = &self.base {
            base.cursor_pos(area)
        } else {
            None
        }
    }

    fn cursor_style(&self, area: ratatui::layout::Rect) -> SetCursorStyle {
        if let Some(top) = self.stack.last() {
            top.cursor_style(area)
        } else if let Some(base) = &self.base {
            base.cursor_style(area)
        } else {
            SetCursorStyle::DefaultUserShape
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEventKind, KeyEventState};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Convenience constructor for a press key event with no modifiers.
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: crossterm::event::KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    // -----------------------------------------------------------------------
    // Mock panes
    // -----------------------------------------------------------------------

    /// A fully configurable mock pane for testing.
    struct MockPane {
        /// What `handle_key_event` returns.
        handle_result: Handled,
        /// What `is_complete` returns.
        complete: bool,
        /// Accumulates every key code this pane received.
        received_keys: Vec<KeyCode>,
        /// What `on_ctrl_c` returns.
        ctrl_c_result: CtrlCAction,
        /// Optional view identifier.
        view_id: Option<&'static str>,
        /// What `desired_height` returns.
        desired_h: u16,
        /// What `cursor_pos` returns.
        cursor: Option<(u16, u16)>,
        /// What `cursor_style` returns.
        cursor_style: SetCursorStyle,
    }

    impl MockPane {
        fn new() -> Self {
            Self {
                handle_result: Handled::Handled,
                complete: false,
                received_keys: Vec::new(),
                ctrl_c_result: CtrlCAction::NotHandled,
                view_id: None,
                desired_h: 5,
                cursor: None,
                cursor_style: SetCursorStyle::DefaultUserShape,
            }
        }

        fn with_handled(mut self, result: Handled) -> Self {
            self.handle_result = result;
            self
        }

        fn with_complete(mut self, complete: bool) -> Self {
            self.complete = complete;
            self
        }

        fn with_ctrl_c(mut self, action: CtrlCAction) -> Self {
            self.ctrl_c_result = action;
            self
        }

        fn with_view_id(mut self, id: &'static str) -> Self {
            self.view_id = Some(id);
            self
        }

        fn with_desired_height(mut self, h: u16) -> Self {
            self.desired_h = h;
            self
        }

        fn with_cursor(mut self, pos: Option<(u16, u16)>) -> Self {
            self.cursor = pos;
            self
        }

        fn with_cursor_style(mut self, style: SetCursorStyle) -> Self {
            self.cursor_style = style;
            self
        }
    }

    impl Renderable for MockPane {
        fn render(&self, _area: Rect, _buf: &mut Buffer) {}
        fn desired_height(&self, _width: u16) -> u16 {
            self.desired_h
        }
        fn cursor_pos(&self, _area: Rect) -> Option<(u16, u16)> {
            self.cursor
        }
        fn cursor_style(&self, _area: Rect) -> SetCursorStyle {
            self.cursor_style
        }
    }

    impl PaneView for MockPane {
        fn handle_key_event(&mut self, ev: KeyEvent) -> Handled {
            self.received_keys.push(ev.code);
            self.handle_result
        }
        fn is_complete(&self) -> bool {
            self.complete
        }
        fn on_ctrl_c(&mut self) -> CtrlCAction {
            self.ctrl_c_result
        }
        fn view_id(&self) -> Option<&'static str> {
            self.view_id
        }
    }

    // -----------------------------------------------------------------------
    // Handled enum
    // -----------------------------------------------------------------------

    #[test]
    fn test_handled_variants_are_distinct() {
        assert_ne!(Handled::Handled, Handled::NotHandled);
    }

    #[test]
    fn test_handled_debug_format() {
        assert_eq!(format!("{:?}", Handled::Handled), "Handled");
        assert_eq!(format!("{:?}", Handled::NotHandled), "NotHandled");
    }

    #[test]
    fn test_handled_clone() {
        let h = Handled::Handled;
        let c = h;
        assert_eq!(h, c);
    }

    // -----------------------------------------------------------------------
    // CtrlCAction enum
    // -----------------------------------------------------------------------

    #[test]
    fn test_ctrl_c_action_variants_are_distinct() {
        assert_ne!(CtrlCAction::NotHandled, CtrlCAction::Handled);
        assert_ne!(CtrlCAction::NotHandled, CtrlCAction::Cancel);
        assert_ne!(CtrlCAction::Handled, CtrlCAction::Cancel);
    }

    #[test]
    fn test_ctrl_c_action_debug_format() {
        assert_eq!(format!("{:?}", CtrlCAction::NotHandled), "NotHandled");
        assert_eq!(format!("{:?}", CtrlCAction::Handled), "Handled");
        assert_eq!(format!("{:?}", CtrlCAction::Cancel), "Cancel");
    }

    // -----------------------------------------------------------------------
    // PaneView trait — default method implementations
    // -----------------------------------------------------------------------

    #[test]
    fn test_pane_view_default_is_complete() {
        struct MinimalPane;
        impl Renderable for MinimalPane {
            fn render(&self, _: Rect, _: &mut Buffer) {}
            fn desired_height(&self, _: u16) -> u16 {
                0
            }
        }
        impl PaneView for MinimalPane {
            fn handle_key_event(&mut self, _: KeyEvent) -> Handled {
                Handled::NotHandled
            }
        }
        assert!(!MinimalPane.is_complete());
    }

    #[test]
    fn test_pane_view_default_on_ctrl_c() {
        struct MinimalPane;
        impl Renderable for MinimalPane {
            fn render(&self, _: Rect, _: &mut Buffer) {}
            fn desired_height(&self, _: u16) -> u16 {
                0
            }
        }
        impl PaneView for MinimalPane {
            fn handle_key_event(&mut self, _: KeyEvent) -> Handled {
                Handled::NotHandled
            }
        }
        assert_eq!(MinimalPane.on_ctrl_c(), CtrlCAction::NotHandled);
    }

    #[test]
    fn test_pane_view_default_view_id() {
        struct MinimalPane;
        impl Renderable for MinimalPane {
            fn render(&self, _: Rect, _: &mut Buffer) {}
            fn desired_height(&self, _: u16) -> u16 {
                0
            }
        }
        impl PaneView for MinimalPane {
            fn handle_key_event(&mut self, _: KeyEvent) -> Handled {
                Handled::NotHandled
            }
        }
        assert_eq!(MinimalPane.view_id(), None);
    }

    #[test]
    fn test_pane_view_handle_key_event_signature() {
        let mut pane = MockPane::new();
        let ev = key(KeyCode::Char('x'));
        let result = pane.handle_key_event(ev);
        assert_eq!(result, Handled::Handled);
        assert_eq!(pane.received_keys, vec![KeyCode::Char('x')]);
    }

    #[test]
    fn test_pane_view_view_id_custom() {
        let pane = MockPane::new().with_view_id("test-pane");
        assert_eq!(pane.view_id(), Some("test-pane"));
    }

    #[test]
    fn test_pane_view_on_ctrl_c_custom() {
        let mut pane = MockPane::new().with_ctrl_c(CtrlCAction::Cancel);
        assert_eq!(pane.on_ctrl_c(), CtrlCAction::Cancel);
    }

    // -----------------------------------------------------------------------
    // PaneStack — construction
    // -----------------------------------------------------------------------

    #[test]
    fn test_pane_stack_new_is_empty() {
        let stack = PaneStack::new();
        assert_eq!(stack.depth(), 0);
        assert!(!stack.is_active());
    }

    #[test]
    fn test_pane_stack_default_equals_new() {
        assert_eq!(PaneStack::default().depth(), PaneStack::new().depth());
        assert!(!PaneStack::default().is_active());
    }

    // -----------------------------------------------------------------------
    // PaneStack — push / pop / depth
    // -----------------------------------------------------------------------

    #[test]
    fn test_pane_stack_push_increases_depth() {
        let mut stack = PaneStack::new();
        stack.push(Box::new(MockPane::new()));
        assert_eq!(stack.depth(), 1);
        assert!(stack.is_active());
    }

    #[test]
    fn test_pane_stack_push_multiple() {
        let mut stack = PaneStack::new();
        stack.push(Box::new(MockPane::new()));
        stack.push(Box::new(MockPane::new()));
        stack.push(Box::new(MockPane::new()));
        assert_eq!(stack.depth(), 3);
    }

    #[test]
    fn test_pane_stack_pop_returns_pane() {
        let mut stack = PaneStack::new();
        let pane = MockPane::new().with_view_id("popped");
        stack.push(Box::new(pane));
        let popped = stack.pop();
        assert!(popped.is_some());
        assert_eq!(popped.unwrap().view_id(), Some("popped"));
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn test_pane_stack_pop_empty_returns_none() {
        let mut stack = PaneStack::new();
        assert!(stack.pop().is_none());
    }

    #[test]
    fn test_pane_stack_pop_fifo_order() {
        let mut stack = PaneStack::new();
        stack.push(Box::new(MockPane::new().with_view_id("first")));
        stack.push(Box::new(MockPane::new().with_view_id("second")));
        stack.push(Box::new(MockPane::new().with_view_id("third")));

        assert_eq!(stack.pop().unwrap().view_id(), Some("third"));
        assert_eq!(stack.pop().unwrap().view_id(), Some("second"));
        assert_eq!(stack.pop().unwrap().view_id(), Some("first"));
        assert!(stack.pop().is_none());
    }

    // -----------------------------------------------------------------------
    // PaneStack — base panel
    // -----------------------------------------------------------------------

    #[test]
    fn test_pane_stack_set_base_makes_active() {
        let mut stack = PaneStack::new();
        assert!(!stack.is_active());
        stack.set_base(Box::new(MockPane::new()));
        assert!(stack.is_active());
        // base does not affect depth
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn test_pane_stack_base_is_fallback_when_stack_empty() {
        let mut stack = PaneStack::new();
        let base = MockPane::new().with_view_id("base");
        stack.set_base(Box::new(base));

        // Stack is empty, base acts as fallback
        // We verify this via handle_key_event routing to base
        let handled = stack.handle_key_event(key(KeyCode::Char('a')));
        assert_eq!(handled, Handled::Handled);
        // Pop the (now-empty) stack — should not affect base
        assert!(stack.pop().is_none());
    }

    #[test]
    fn test_pane_stack_stack_takes_priority_over_base() {
        let mut stack = PaneStack::new();
        stack.set_base(Box::new(MockPane::new().with_view_id("base")));
        let overlay = MockPane::new().with_view_id("overlay");
        stack.push(Box::new(overlay));

        // Stack top should be "overlay"
        let popped = stack.pop().unwrap();
        assert_eq!(popped.view_id(), Some("overlay"));

        // Base is still there
        assert!(stack.is_active());
    }

    // -----------------------------------------------------------------------
    // PaneStack — handle_key_event routing
    // -----------------------------------------------------------------------

    #[test]
    fn test_pane_stack_handle_key_routes_to_active() {
        let mut stack = PaneStack::new();
        let mut pane = MockPane::new().with_handled(Handled::Handled);
        pane.received_keys.clear(); // ensure clean state
        stack.push(Box::new(pane));

        let result = stack.handle_key_event(key(KeyCode::Char('x')));
        assert_eq!(result, Handled::Handled);
    }

    #[test]
    fn test_pane_stack_handle_key_returns_not_handled_when_not_handled() {
        let mut stack = PaneStack::new();
        stack.push(Box::new(MockPane::new().with_handled(Handled::NotHandled)));
        let result = stack.handle_key_event(key(KeyCode::Char('x')));
        assert_eq!(result, Handled::NotHandled);
    }

    #[test]
    fn test_pane_stack_handle_key_returns_not_handled_when_empty() {
        let mut stack = PaneStack::new();
        let result = stack.handle_key_event(key(KeyCode::Char('x')));
        assert_eq!(result, Handled::NotHandled);
    }

    #[test]
    fn test_pane_stack_handle_key_routes_to_base_when_stack_empty() {
        let mut stack = PaneStack::new();
        stack.set_base(Box::new(MockPane::new().with_handled(Handled::Handled)));
        let result = stack.handle_key_event(key(KeyCode::Esc));
        assert_eq!(result, Handled::Handled);
    }

    #[test]
    fn test_pane_stack_handle_key_does_not_route_to_base_when_stack_active() {
        let mut stack = PaneStack::new();
        let base = MockPane::new().with_view_id("base");
        stack.set_base(Box::new(base));
        // Stack overlay handles it
        stack.push(Box::new(MockPane::new().with_handled(Handled::NotHandled)));
        let result = stack.handle_key_event(key(KeyCode::Char('x')));
        // Even if stack overlay doesn't handle, it was still routed to it
        assert_eq!(result, Handled::NotHandled);
    }

    // -----------------------------------------------------------------------
    // PaneStack — cleanup_completed (auto-pop)
    // -----------------------------------------------------------------------

    #[test]
    fn test_pane_stack_cleanup_removes_completed_panes() {
        let mut stack = PaneStack::new();
        stack.push(Box::new(MockPane::new().with_complete(true)));
        stack.push(Box::new(MockPane::new().with_complete(true)));

        // Trigger cleanup via handle_key_event
        stack.handle_key_event(key(KeyCode::Char('x')));

        // Both completed panes should be popped
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn test_pane_stack_cleanup_stops_at_non_completed() {
        let mut stack = PaneStack::new();
        stack.push(Box::new(MockPane::new().with_complete(false)));
        stack.push(Box::new(MockPane::new().with_complete(true)));
        stack.push(Box::new(MockPane::new().with_complete(true)));

        // Only the top 2 completed panes should be popped
        stack.handle_key_event(key(KeyCode::Char('x')));
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn test_pane_stack_cleanup_does_not_remove_incomplete() {
        let mut stack = PaneStack::new();
        stack.push(Box::new(MockPane::new().with_complete(false)));
        stack.handle_key_event(key(KeyCode::Char('x')));
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn test_pane_stack_cleanup_handles_mixed_completion() {
        let mut stack = PaneStack::new();
        // Bottom: incomplete, middle: complete, top: incomplete
        stack.push(Box::new(MockPane::new().with_complete(false)));
        stack.push(Box::new(MockPane::new().with_complete(true)));
        stack.push(Box::new(MockPane::new().with_complete(false)));

        stack.handle_key_event(key(KeyCode::Char('x')));
        // Only the top incomplete should remain, but the middle complete is
        // blocked by the top incomplete.
        assert_eq!(stack.depth(), 3);
    }

    #[test]
    fn test_pane_stack_cleanup_only_after_handled() {
        let mut stack = PaneStack::new();
        let pane = MockPane::new()
            .with_handled(Handled::NotHandled)
            .with_complete(true);
        stack.push(Box::new(pane));

        // Event not handled, so cleanup should NOT run
        stack.handle_key_event(key(KeyCode::Char('x')));
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn test_pane_stack_cleanup_chain_completion() {
        // Scenario: completing a pane reveals the next one which is also complete
        let mut stack = PaneStack::new();
        stack.push(Box::new(MockPane::new().with_complete(true)));
        stack.push(Box::new(MockPane::new().with_complete(true)));

        // The active pane is the top one (complete=true), so after handling
        // it gets popped, revealing the next one (also complete=true), which
        // also gets popped — chain cleanup.
        stack.handle_key_event(key(KeyCode::Char('x')));
        assert_eq!(stack.depth(), 0);
    }

    // -----------------------------------------------------------------------
    // PaneStack — Renderable delegation
    // -----------------------------------------------------------------------

    #[test]
    fn test_pane_stack_render_delegates_to_stack_top() {
        let mut stack = PaneStack::new();
        // Cursor is the only thing we can observe from this mock (render is
        // a no-op), but we can verify that the stack top's methods are called.
        let top = MockPane::new().with_cursor(Some((3, 5)));
        stack.push(Box::new(top));

        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(stack.cursor_pos(area), Some((3, 5)));
    }

    #[test]
    fn test_pane_stack_render_falls_back_to_base() {
        let mut stack = PaneStack::new();
        let base = MockPane::new().with_cursor(Some((1, 2)));
        stack.set_base(Box::new(base));

        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(stack.cursor_pos(area), Some((1, 2)));
    }

    #[test]
    fn test_pane_stack_render_returns_none_when_empty() {
        let stack = PaneStack::new();
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(stack.cursor_pos(area), None);
    }

    #[test]
    fn test_pane_stack_desired_height_delegates_to_stack_top() {
        let mut stack = PaneStack::new();
        stack.push(Box::new(MockPane::new().with_desired_height(10)));
        assert_eq!(stack.desired_height(80), 10);
    }

    #[test]
    fn test_pane_stack_desired_height_falls_back_to_base() {
        let mut stack = PaneStack::new();
        stack.set_base(Box::new(MockPane::new().with_desired_height(7)));
        assert_eq!(stack.desired_height(80), 7);
    }

    #[test]
    fn test_pane_stack_desired_height_returns_zero_when_empty() {
        let stack = PaneStack::new();
        assert_eq!(stack.desired_height(80), 0);
    }

    #[test]
    fn test_pane_stack_desired_height_ignores_base_when_stack_active() {
        let mut stack = PaneStack::new();
        stack.set_base(Box::new(MockPane::new().with_desired_height(100)));
        stack.push(Box::new(MockPane::new().with_desired_height(3)));
        assert_eq!(stack.desired_height(80), 3);
    }

    #[test]
    fn test_pane_stack_cursor_style_delegates_to_stack_top() {
        let mut stack = PaneStack::new();
        stack.push(Box::new(
            MockPane::new().with_cursor_style(SetCursorStyle::BlinkingBar),
        ));
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(
            stack.cursor_style(area),
            SetCursorStyle::BlinkingBar
        );
    }

    #[test]
    fn test_pane_stack_cursor_style_falls_back_to_base() {
        let mut stack = PaneStack::new();
        stack.set_base(Box::new(
            MockPane::new().with_cursor_style(SetCursorStyle::SteadyBlock),
        ));
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(
            stack.cursor_style(area),
            SetCursorStyle::SteadyBlock
        );
    }

    #[test]
    fn test_pane_stack_cursor_style_default_when_empty() {
        let stack = PaneStack::new();
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(
            stack.cursor_style(area),
            SetCursorStyle::DefaultUserShape
        );
    }

    #[test]
    fn test_pane_stack_cursor_style_ignores_base_when_stack_active() {
        let mut stack = PaneStack::new();
        stack.set_base(Box::new(
            MockPane::new().with_cursor_style(SetCursorStyle::SteadyBlock),
        ));
        stack.push(Box::new(
            MockPane::new().with_cursor_style(SetCursorStyle::BlinkingBar),
        ));
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(
            stack.cursor_style(area),
            SetCursorStyle::BlinkingBar
        );
    }

    // -----------------------------------------------------------------------
    // PaneStack — edge cases and boundary conditions
    // -----------------------------------------------------------------------

    #[test]
    fn test_pane_stack_desired_height_zero_width() {
        let mut stack = PaneStack::new();
        stack.push(Box::new(MockPane::new().with_desired_height(5)));
        // Zero width should still return the pane's desired height
        // (the mock ignores width, so it just returns 5)
        assert_eq!(stack.desired_height(0), 5);
    }

    #[test]
    fn test_pane_stack_desired_height_max_width() {
        let mut stack = PaneStack::new();
        stack.push(Box::new(MockPane::new().with_desired_height(5)));
        assert_eq!(stack.desired_height(u16::MAX), 5);
    }

    #[test]
    fn test_pane_stack_large_depth_does_not_panic() {
        let mut stack = PaneStack::new();
        for _ in 0..10_000 {
            stack.push(Box::new(MockPane::new()));
        }
        assert_eq!(stack.depth(), 10_000);
        // Pop all back
        for _ in 0..10_000 {
            stack.pop();
        }
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn test_pane_stack_handle_key_does_not_crash_with_many_panes() {
        let mut stack = PaneStack::new();
        for _ in 0..100 {
            stack.push(Box::new(
                MockPane::new().with_handled(Handled::NotHandled),
            ));
        }
        // All 100 panes get a chance, but the top one gets it first
        let result = stack.handle_key_event(key(KeyCode::Char('a')));
        assert_eq!(result, Handled::NotHandled);
    }

    #[test]
    fn test_pane_stack_cleanup_does_not_panic_on_empty() {
        let mut stack = PaneStack::new();
        stack.cleanup_completed(); // should not panic
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn test_pane_stack_handle_key_many_events() {
        let mut stack = PaneStack::new();
        let keys = 1000;
        // A pane that handles every other event
        // We use a real MockPane to track received keys
        let pane = MockPane::new().with_handled(Handled::Handled);
        stack.push(Box::new(pane));

        for i in 0..keys {
            let code = if i % 2 == 0 {
                KeyCode::Char('a')
            } else {
                KeyCode::Char('b')
            };
            stack.handle_key_event(key(code));
        }

        // The pane should still be on the stack (not completed)
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn test_pane_stack_is_active_after_push_pop() {
        let mut stack = PaneStack::new();
        assert!(!stack.is_active());
        stack.push(Box::new(MockPane::new()));
        assert!(stack.is_active());
        stack.pop();
        assert!(!stack.is_active());
    }

    #[test]
    fn test_pane_stack_is_active_with_base_after_pop() {
        let mut stack = PaneStack::new();
        stack.set_base(Box::new(MockPane::new()));
        assert!(stack.is_active());
        stack.push(Box::new(MockPane::new()));
        assert!(stack.is_active());
        stack.pop();
        // Base is still there
        assert!(stack.is_active());
    }

    #[test]
    fn test_pane_stack_does_not_consume_events_when_base_only_and_not_handled() {
        let mut stack = PaneStack::new();
        stack.set_base(Box::new(
            MockPane::new().with_handled(Handled::NotHandled),
        ));
        let result = stack.handle_key_event(key(KeyCode::Char('x')));
        assert_eq!(result, Handled::NotHandled);
    }

    #[test]
    fn test_pane_stack_consumes_events_when_base_handles() {
        let mut stack = PaneStack::new();
        stack.set_base(Box::new(
            MockPane::new().with_handled(Handled::Handled),
        ));
        let result = stack.handle_key_event(key(KeyCode::Char('x')));
        assert_eq!(result, Handled::Handled);
    }

    // -----------------------------------------------------------------------
    // PaneStack — Renderable trait method signatures
    // -----------------------------------------------------------------------

    #[test]
    fn test_pane_stack_render_signature_accepts_rect_and_buffer() {
        let mut stack = PaneStack::new();
        stack.push(Box::new(MockPane::new()));
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        // Should not panic
        stack.render(area, &mut buf);
    }

    #[test]
    fn test_pane_stack_render_no_panic_on_empty() {
        let stack = PaneStack::new();
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        stack.render(area, &mut buf);
        // No crash
    }

    #[test]
    fn test_pane_stack_render_no_panic_on_zero_area() {
        let mut stack = PaneStack::new();
        stack.push(Box::new(MockPane::new()));
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        stack.render(area, &mut buf);
        // No crash
    }

    // -----------------------------------------------------------------------
    // PaneView trait — Renderable default methods
    // -----------------------------------------------------------------------

    #[test]
    fn test_pane_view_renderable_default_cursor_pos() {
        struct MinimalPane;
        impl Renderable for MinimalPane {
            fn render(&self, _: Rect, _: &mut Buffer) {}
            fn desired_height(&self, _: u16) -> u16 {
                0
            }
        }
        impl PaneView for MinimalPane {
            fn handle_key_event(&mut self, _: KeyEvent) -> Handled {
                Handled::NotHandled
            }
        }
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(MinimalPane.cursor_pos(area), None);
    }

    #[test]
    fn test_pane_view_renderable_default_cursor_style() {
        struct MinimalPane;
        impl Renderable for MinimalPane {
            fn render(&self, _: Rect, _: &mut Buffer) {}
            fn desired_height(&self, _: u16) -> u16 {
                0
            }
        }
        impl PaneView for MinimalPane {
            fn handle_key_event(&mut self, _: KeyEvent) -> Handled {
                Handled::NotHandled
            }
        }
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(
            MinimalPane.cursor_style(area),
            SetCursorStyle::DefaultUserShape
        );
    }
}