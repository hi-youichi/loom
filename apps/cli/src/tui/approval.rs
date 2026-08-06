//! Approval overlay — prompts the user to approve or deny an action.
//!
//! # Overview
//!
//! - [`ApprovalRequest`] — the thing being approved (command, file edit, tool call).
//! - [`ApprovalResult`] — the user's decision.
//! - [`ApprovalOverlay`] — a [`PaneView`] that renders the approval dialog and
//!   handles Y/N/D/A keyboard input.
//!
//! # Key bindings
//!
//! | Key | Action |
//! |-----|--------|
//! | `Y` | Allow once |
//! | `N` / `Esc` | Deny |
//! | `A` | Always allow (session) |
//! | `D` | Toggle diff display |
//! | `Ctrl+C` | Deny and close |

use crossterm::cursor::SetCursorStyle;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::{Stylize, Widget},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

use super::pane::{CtrlCAction, Handled, PaneView};
use super::render::Renderable;

// ---------------------------------------------------------------------------
// ApprovalRequest
// ---------------------------------------------------------------------------

/// The kind of action that requires user approval.
#[derive(Debug, Clone)]
pub enum ApprovalRequest {
    /// A shell command to execute.
    Command {
        /// The command string.
        command: String,
        /// A human-readable description of what the command does.
        description: String,
    },
    /// A file edit (diff) to apply.
    FileEdit {
        /// Path to the file being modified.
        file_path: String,
        /// The unified diff text.
        diff: String,
    },
    /// A tool invocation.
    ToolCall {
        /// Name of the tool being called.
        tool_name: String,
        /// Arguments passed to the tool, as JSON.
        args: serde_json::Value,
    },
}

// ---------------------------------------------------------------------------
// ApprovalResult
// ---------------------------------------------------------------------------

/// The user's decision on an approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalResult {
    /// Allow the action once.
    Allow,
    /// Deny the action.
    Deny,
    /// Allow this action for the rest of the session (always allow).
    AlwaysAllow,
    /// Show the diff (only meaningful for file edits).
    ShowDiff,
}

// ---------------------------------------------------------------------------
// ApprovalOverlay
// ---------------------------------------------------------------------------

/// A modal overlay that presents an approval request to the user.
///
/// The overlay renders a bordered block with the request details and a
/// key-binding hint bar at the bottom. The user responds with Y/N/A/D or
/// Esc to dismiss.
pub struct ApprovalOverlay {
    /// The request being presented.
    request: ApprovalRequest,
    /// The user's decision, if made.
    result: Option<ApprovalResult>,
    /// Whether the diff view is currently expanded.
    show_diff: bool,
    /// An optional error message to display.
    error: Option<String>,
}

impl ApprovalOverlay {
    /// Create a new approval overlay for the given request.
    pub fn new(request: ApprovalRequest) -> Self {
        Self {
            request,
            result: None,
            show_diff: false,
            error: None,
        }
    }

    /// The user's decision, if one has been made.
    pub fn result(&self) -> Option<ApprovalResult> {
        self.result
    }

    /// Set an error message to display on the overlay.
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// The approval request being presented.
    pub fn request(&self) -> &ApprovalRequest {
        &self.request
    }

    /// Whether the diff view is expanded.
    pub fn show_diff(&self) -> bool {
        self.show_diff
    }
}

// ---------------------------------------------------------------------------
// PaneView impl
// ---------------------------------------------------------------------------

impl PaneView for ApprovalOverlay {
    fn handle_key_event(&mut self, key: KeyEvent) -> Handled {
        match key.code {
            KeyCode::Char(c) => match c.to_ascii_lowercase() {
                'y' => {
                    self.result = Some(ApprovalResult::Allow);
                    Handled::Handled
                }
                'n' => {
                    self.result = Some(ApprovalResult::Deny);
                    Handled::Handled
                }
                'a' => {
                    self.result = Some(ApprovalResult::AlwaysAllow);
                    Handled::Handled
                }
                'd' => {
                    self.show_diff = !self.show_diff;
                    Handled::Handled
                }
                _ => Handled::NotHandled,
            },
            KeyCode::Esc => {
                self.result = Some(ApprovalResult::Deny);
                Handled::Handled
            }
            _ => Handled::NotHandled,
        }
    }

    fn is_complete(&self) -> bool {
        self.result.is_some()
    }

    fn on_ctrl_c(&mut self) -> CtrlCAction {
        self.result = Some(ApprovalResult::Deny);
        CtrlCAction::Handled
    }

    fn view_id(&self) -> Option<&'static str> {
        Some("approval")
    }
}

// ---------------------------------------------------------------------------
// Renderable impl
// ---------------------------------------------------------------------------

impl Renderable for ApprovalOverlay {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        // ── Outer border block ──────────────────────────────────────────
        let border_style = Style::default().fg(Color::Yellow);
        let title_style = Style::default()
            .fg(Color::Yellow)
            .bold();

        let block = Block::default()
            .title(Line::from(vec![
                Span::styled(" 审批 ", title_style),
            ]))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style);

        // Inner area (skip the border)
        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );

        // ── Request content ─────────────────────────────────────────────
        let content_lines = self.build_content_lines();
        let content_height = content_lines.len() as u16;

        // Render the content paragraph
        let content_text = Text::from(content_lines);
        let content_area = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            content_height.min(inner.height),
        );
        let content_paragraph = Paragraph::new(content_text)
            .wrap(Wrap { trim: false });
        content_paragraph.render(content_area, buf);

        // ── Error message ───────────────────────────────────────────────
        if let Some(err) = &self.error {
            let err_area = Rect::new(
                inner.x,
                inner.y + content_height,
                inner.width,
                1,
            );
            let err_line = Paragraph::new(Line::from(vec![
                Span::styled("⚠ ", Style::default().fg(Color::Red)),
                Span::styled(err, Style::default().fg(Color::Red)),
            ]));
            err_line.render(err_area, buf);
        }

        // ── Hint bar at the bottom ──────────────────────────────────────
        let hint_y = inner.y + inner.height.saturating_sub(1);
        let hint_style = Style::default().fg(Color::DarkGray);

        let hint_text = if self.show_diff {
            " [Y] 允许  [N] 拒绝  [D] 关闭差异  [A] 始终允许  [Esc] 取消 "
        } else {
            " [Y] 允许  [N] 拒绝  [D] 显示差异  [A] 始终允许  [Esc] 取消 "
        };

        let hint = Paragraph::new(Line::from(Span::styled(hint_text, hint_style)));
        let hint_area = Rect::new(area.x, hint_y, area.width, 1);
        hint.render(hint_area, buf);

        // ── Render the border block on top ──────────────────────────────
        // (Render block last so the title and borders are on top of content)
        block.render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        let content_lines = match &self.request {
            ApprovalRequest::Command { command, description } => {
                description.lines().count()
                    + command.lines().count()
                    + 2 // blank line + "命令:" label
            }
            ApprovalRequest::FileEdit { file_path: _, diff } => {
                // "文件: <path>" + blank line + diff lines
                let header = 2; // file path line + separator
                header + diff.lines().count()
            }
            ApprovalRequest::ToolCall { tool_name: _, args } => {
                // "工具: <name>" + "参数: <json>" + potential extra lines
                let args_text = serde_json::to_string_pretty(args)
                    .unwrap_or_else(|_| args.to_string());
                2 + args_text.lines().count()
            }
        };
        // Add borders (2), hint bar (1), error (1 if present), gap (1)
        let extra = 4 + if self.error.is_some() { 1 } else { 0 };
        (content_lines as u16 + extra).min(24)
    }

    fn cursor_style(&self, _area: Rect) -> SetCursorStyle {
        // No cursor for the approval overlay — it's a modal dialog.
        SetCursorStyle::DefaultUserShape
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

impl ApprovalOverlay {
    /// Build the text lines for the request content body.
    fn build_content_lines(&self) -> Vec<Line<'static>> {
        match &self.request {
            ApprovalRequest::Command { command, description } => {
                vec![
                    Line::from(Span::styled(
                        "📋 命令执行",
                        Style::default().fg(Color::Cyan).bold(),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        description.clone(),
                        Style::default().fg(Color::White),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("命令: ", Style::default().fg(Color::Yellow)),
                        Span::styled(command.clone(), Style::default().fg(Color::White)),
                    ]),
                ]
            }
            ApprovalRequest::FileEdit { file_path, diff } => {
                let mut lines: Vec<Line<'static>> = vec![
                    Line::from(Span::styled(
                        "📝 文件修改",
                        Style::default().fg(Color::Cyan).bold(),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("文件: ", Style::default().fg(Color::Yellow)),
                        Span::styled(file_path.clone(), Style::default().fg(Color::White)),
                    ]),
                    Line::from(""),
                ];

                if self.show_diff {
                    for line in diff.lines() {
                        let style = if line.starts_with('+') {
                            Style::default().fg(Color::Green)
                        } else if line.starts_with('-') {
                            Style::default().fg(Color::Red)
                        } else if line.starts_with("@@") {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        };
                        lines.push(Line::from(Span::styled(line.to_string(), style)));
                    }
                } else {
                    let preview_lines: Vec<&str> = diff.lines().take(5).collect();
                    for line in &preview_lines {
                        let style = if line.starts_with('+') {
                            Style::default().fg(Color::Green)
                        } else if line.starts_with('-') {
                            Style::default().fg(Color::Red)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        };
                        lines.push(Line::from(Span::styled(line.to_string(), style)));
                    }
                    let total = diff.lines().count();
                    if total > 5 {
                        lines.push(Line::from(Span::styled(
                            format!("  ... 还有 {} 行变更 (按 D 查看全部)", total - 5),
                            Style::default().fg(Color::DarkGray).italic(),
                        )));
                    }
                }

                lines
            }
            ApprovalRequest::ToolCall { tool_name, args } => {
                let args_text = serde_json::to_string_pretty(args)
                    .unwrap_or_else(|_| args.to_string());
                let mut lines: Vec<Line<'static>> = vec![
                    Line::from(Span::styled(
                        "🛠 工具调用",
                        Style::default().fg(Color::Cyan).bold(),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("工具: ", Style::default().fg(Color::Yellow)),
                        Span::styled(tool_name.clone(), Style::default().fg(Color::White)),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled("参数:", Style::default().fg(Color::Yellow))),
                ];
                for line in args_text.lines() {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                lines
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
    use crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    // -----------------------------------------------------------------------
    // ApprovalRequest construction
    // -----------------------------------------------------------------------

    #[test]
    fn test_command_request() {
        let req = ApprovalRequest::Command {
            command: "ls -la".into(),
            description: "List directory contents".into(),
        };
        match req {
            ApprovalRequest::Command { command, description } => {
                assert_eq!(command, "ls -la");
                assert_eq!(description, "List directory contents");
            }
            _ => panic!("Expected Command variant"),
        }
    }

    #[test]
    fn test_file_edit_request() {
        let req = ApprovalRequest::FileEdit {
            file_path: "src/main.rs".into(),
            diff: "+println!(\"hello\")".into(),
        };
        match req {
            ApprovalRequest::FileEdit { file_path, diff } => {
                assert_eq!(file_path, "src/main.rs");
                assert_eq!(diff, "+println!(\"hello\")");
            }
            _ => panic!("Expected FileEdit variant"),
        }
    }

    #[test]
    fn test_tool_call_request() {
        let req = ApprovalRequest::ToolCall {
            tool_name: "bash".into(),
            args: serde_json::json!({"command": "echo hi"}),
        };
        match req {
            ApprovalRequest::ToolCall { tool_name, args } => {
                assert_eq!(tool_name, "bash");
                assert_eq!(args["command"], "echo hi");
            }
            _ => panic!("Expected ToolCall variant"),
        }
    }

    // -----------------------------------------------------------------------
    // ApprovalResult values
    // -----------------------------------------------------------------------

    #[test]
    fn test_approval_result_variants_are_distinct() {
        assert_ne!(ApprovalResult::Allow, ApprovalResult::Deny);
        assert_ne!(ApprovalResult::Allow, ApprovalResult::AlwaysAllow);
        assert_ne!(ApprovalResult::Allow, ApprovalResult::ShowDiff);
        assert_ne!(ApprovalResult::Deny, ApprovalResult::AlwaysAllow);
        assert_ne!(ApprovalResult::Deny, ApprovalResult::ShowDiff);
        assert_ne!(ApprovalResult::AlwaysAllow, ApprovalResult::ShowDiff);
    }

    #[test]
    fn test_approval_result_debug() {
        assert_eq!(format!("{:?}", ApprovalResult::Allow), "Allow");
        assert_eq!(format!("{:?}", ApprovalResult::Deny), "Deny");
        assert_eq!(format!("{:?}", ApprovalResult::AlwaysAllow), "AlwaysAllow");
        assert_eq!(format!("{:?}", ApprovalResult::ShowDiff), "ShowDiff");
    }

    // -----------------------------------------------------------------------
    // ApprovalOverlay construction
    // -----------------------------------------------------------------------

    #[test]
    fn test_approval_overlay_new() {
        let req = ApprovalRequest::Command {
            command: "echo test".into(),
            description: "Test command".into(),
        };
        let overlay = ApprovalOverlay::new(req);
        assert!(overlay.result().is_none());
        assert!(!overlay.show_diff());
        assert!(overlay.error.is_none());
        assert!(!overlay.is_complete());
    }

    #[test]
    fn test_approval_overlay_view_id() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let overlay = ApprovalOverlay::new(req);
        assert_eq!(overlay.view_id(), Some("approval"));
    }

    // -----------------------------------------------------------------------
    // Key handling — Y = Allow
    // -----------------------------------------------------------------------

    #[test]
    fn test_key_y_allow() {
        let req = ApprovalRequest::Command {
            command: "echo hi".into(),
            description: "Say hi".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        let result = overlay.handle_key_event(key(KeyCode::Char('y')));
        assert_eq!(result, Handled::Handled);
        assert_eq!(overlay.result(), Some(ApprovalResult::Allow));
        assert!(overlay.is_complete());
    }

    #[test]
    fn test_key_uppercase_y_allow() {
        let req = ApprovalRequest::Command {
            command: "echo hi".into(),
            description: "Say hi".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        let result = overlay.handle_key_event(key(KeyCode::Char('Y')));
        assert_eq!(result, Handled::Handled);
        assert_eq!(overlay.result(), Some(ApprovalResult::Allow));
    }

    // -----------------------------------------------------------------------
    // Key handling — N = Deny
    // -----------------------------------------------------------------------

    #[test]
    fn test_key_n_deny() {
        let req = ApprovalRequest::Command {
            command: "rm -rf /".into(),
            description: "Dangerous".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        let result = overlay.handle_key_event(key(KeyCode::Char('n')));
        assert_eq!(result, Handled::Handled);
        assert_eq!(overlay.result(), Some(ApprovalResult::Deny));
        assert!(overlay.is_complete());
    }

    // -----------------------------------------------------------------------
    // Key handling — Esc = Deny
    // -----------------------------------------------------------------------

    #[test]
    fn test_key_esc_deny() {
        let req = ApprovalRequest::Command {
            command: "rm -rf /".into(),
            description: "Dangerous".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        let result = overlay.handle_key_event(key(KeyCode::Esc));
        assert_eq!(result, Handled::Handled);
        assert_eq!(overlay.result(), Some(ApprovalResult::Deny));
        assert!(overlay.is_complete());
    }

    // -----------------------------------------------------------------------
    // Key handling — A = AlwaysAllow
    // -----------------------------------------------------------------------

    #[test]
    fn test_key_a_always_allow() {
        let req = ApprovalRequest::Command {
            command: "cargo build".into(),
            description: "Build project".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        let result = overlay.handle_key_event(key(KeyCode::Char('a')));
        assert_eq!(result, Handled::Handled);
        assert_eq!(overlay.result(), Some(ApprovalResult::AlwaysAllow));
        assert!(overlay.is_complete());
    }

    // -----------------------------------------------------------------------
    // Key handling — D = toggle diff
    // -----------------------------------------------------------------------

    #[test]
    fn test_key_d_toggle_diff() {
        let req = ApprovalRequest::FileEdit {
            file_path: "test.rs".into(),
            diff: "+line1\n-line2\n context".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        assert!(!overlay.show_diff());

        // First press: toggle on
        let result = overlay.handle_key_event(key(KeyCode::Char('d')));
        assert_eq!(result, Handled::Handled);
        assert!(overlay.show_diff());
        assert!(!overlay.is_complete()); // toggle doesn't complete

        // Second press: toggle off
        let result = overlay.handle_key_event(key(KeyCode::Char('d')));
        assert_eq!(result, Handled::Handled);
        assert!(!overlay.show_diff());
        assert!(!overlay.is_complete());
    }

    // -----------------------------------------------------------------------
    // Key handling — unknown key = NotHandled
    // -----------------------------------------------------------------------

    #[test]
    fn test_key_unknown_not_handled() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        let result = overlay.handle_key_event(key(KeyCode::Char('z')));
        assert_eq!(result, Handled::NotHandled);
        assert!(overlay.result().is_none());
    }

    #[test]
    fn test_key_non_char_not_handled() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        let result = overlay.handle_key_event(key(KeyCode::Enter));
        assert_eq!(result, Handled::NotHandled);
        assert!(overlay.result().is_none());
    }

    // -----------------------------------------------------------------------
    // Ctrl+C handling
    // -----------------------------------------------------------------------

    #[test]
    fn test_ctrl_c_sets_deny_and_handled() {
        let req = ApprovalRequest::Command {
            command: "rm -rf".into(),
            description: "Danger".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        let action = overlay.on_ctrl_c();
        assert_eq!(action, CtrlCAction::Handled);
        assert_eq!(overlay.result(), Some(ApprovalResult::Deny));
        assert!(overlay.is_complete());
    }

    // -----------------------------------------------------------------------
    // set_error
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_error() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        assert!(overlay.error.is_none());
        overlay.set_error("Something went wrong".into());
        assert_eq!(overlay.error.as_deref(), Some("Something went wrong"));
    }

    // -----------------------------------------------------------------------
    // request accessor
    // -----------------------------------------------------------------------

    #[test]
    fn test_request_accessor() {
        let req = ApprovalRequest::Command {
            command: "cargo test".into(),
            description: "Run tests".into(),
        };
        let overlay = ApprovalOverlay::new(req);
        match overlay.request() {
            ApprovalRequest::Command { command, description } => {
                assert_eq!(command, "cargo test");
                assert_eq!(description, "Run tests");
            }
            _ => panic!("Expected Command"),
        }
    }

    // -----------------------------------------------------------------------
    // Renderable — desired_height sanity
    // -----------------------------------------------------------------------

    #[test]
    fn test_desired_height_command() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let overlay = ApprovalOverlay::new(req);
        let h = overlay.desired_height(80);
        // 2 desc lines + 1 cmd + 2 labels + 4 borders/hint = at least 8
        assert!(h >= 8, "height {h} should be >= 8");
        assert!(h <= 24, "height {h} should be <= 24");
    }

    #[test]
    fn test_desired_height_file_edit() {
        let req = ApprovalRequest::FileEdit {
            file_path: "foo.rs".into(),
            diff: "+a\n-b\n".into(),
        };
        let overlay = ApprovalOverlay::new(req);
        let h = overlay.desired_height(80);
        // 2 header + 2 diff lines + 4 borders/hint = 8
        assert!(h >= 8, "height {h} should be >= 8");
    }

    #[test]
    fn test_desired_height_tool_call() {
        let req = ApprovalRequest::ToolCall {
            tool_name: "bash".into(),
            args: serde_json::json!({"cmd": "echo hi"}),
        };
        let overlay = ApprovalOverlay::new(req);
        let h = overlay.desired_height(80);
        assert!(h >= 9);
    }

    // -----------------------------------------------------------------------
    // Renderable — cursor methods
    // -----------------------------------------------------------------------

    #[test]
    fn test_no_cursor() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let overlay = ApprovalOverlay::new(req);
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(overlay.cursor_pos(area), None);
        assert_eq!(
            overlay.cursor_style(area),
            SetCursorStyle::DefaultUserShape
        );
    }

    // -----------------------------------------------------------------------
    // Boundary conditions — empty / zero / max
    // -----------------------------------------------------------------------

    #[test]
    fn test_empty_command() {
        let req = ApprovalRequest::Command {
            command: "".into(),
            description: "".into(),
        };
        let overlay = ApprovalOverlay::new(req);
        // Should not panic for any trait method
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(overlay.cursor_pos(area), None);
        // desired_height: 0 lines + 0 lines + 2 + 4 = 6
        let h = overlay.desired_height(80);
        assert_eq!(h, 6, "empty command should produce minimal height");
    }

    #[test]
    fn test_empty_file_edit() {
        let req = ApprovalRequest::FileEdit {
            file_path: "".into(),
            diff: "".into(),
        };
        let overlay = ApprovalOverlay::new(req);
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(overlay.cursor_pos(area), None);
        // desired_height: 2 header + 0 diff lines + 4 = 6
        let h = overlay.desired_height(80);
        assert_eq!(h, 6, "empty file edit should produce minimal height");
    }

    #[test]
    fn test_empty_tool_call() {
        let req = ApprovalRequest::ToolCall {
            tool_name: "".into(),
            args: serde_json::Value::Null,
        };
        let overlay = ApprovalOverlay::new(req);
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(overlay.cursor_pos(area), None);
        // null args → to_string_pretty returns "null" (1 line), so 2 + 1 + 4 = 7
        let h = overlay.desired_height(80);
        assert_eq!(h, 7, "null-arg tool call should produce minimal height");
    }

    #[test]
    fn test_zero_width_area() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let overlay = ApprovalOverlay::new(req);
        // desired_height ignores width, so this should return same value
        let h = overlay.desired_height(0);
        assert_eq!(h, 8, "desired_height should ignore zero width");
    }

    #[test]
    fn test_desired_height_capped_at_24() {
        let huge = "x\n".repeat(100);
        let req = ApprovalRequest::Command {
            command: huge.clone(),
            description: huge,
        };
        let overlay = ApprovalOverlay::new(req);
        let h = overlay.desired_height(80);
        assert_eq!(h, 24, "desired_height should be capped at 24");
    }

    #[test]
    fn test_desired_height_capped_at_24_file_edit() {
        let huge_diff = "+++line\n".repeat(100);
        let req = ApprovalRequest::FileEdit {
            file_path: "foo.rs".into(),
            diff: huge_diff,
        };
        let overlay = ApprovalOverlay::new(req);
        let h = overlay.desired_height(80);
        assert_eq!(h, 24, "file edit desired_height should be capped at 24");
    }

    #[test]
    fn test_desired_height_capped_at_24_tool_call() {
        // Build a JSON object with 100 keys so pretty-print produces ~100 lines
        let mut obj = serde_json::Map::new();
        for i in 0..100 {
            obj.insert(format!("key_{i}"), serde_json::json!({"nested": i, "value": format!("v_{i}")}));
        }
        let huge_args = serde_json::Value::Object(obj);
        let req = ApprovalRequest::ToolCall {
            tool_name: "bash".into(),
            args: huge_args,
        };
        let overlay = ApprovalOverlay::new(req);
        let h = overlay.desired_height(80);
        assert_eq!(h, 24, "tool call desired_height should be capped at 24");
    }

    #[test]
    fn test_desired_height_with_error() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        // No error: 1+1+2 + 4 = 8
        assert_eq!(overlay.desired_height(80), 8);
        overlay.set_error("Permission denied".into());
        // With error: 1+1+2 + 5 = 9
        assert_eq!(overlay.desired_height(80), 9, "error should add 1 to height");
    }

    #[test]
    fn test_empty_diff_toggle() {
        let req = ApprovalRequest::FileEdit {
            file_path: "empty.rs".into(),
            diff: "".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        assert!(!overlay.show_diff());
        // Toggle show_diff on — should not panic
        overlay.handle_key_event(key(KeyCode::Char('d')));
        assert!(overlay.show_diff());
        // is_complete should still be false
        assert!(!overlay.is_complete());
    }

    #[test]
    fn test_multi_line_command() {
        let req = ApprovalRequest::Command {
            command: "cargo build\ncargo test\ncargo clippy".into(),
            description: "Build project\nRun tests\nRun linter".into(),
        };
        let overlay = ApprovalOverlay::new(req);
        // 3 + 3 + 2 + 4 = 12
        let h = overlay.desired_height(80);
        assert_eq!(h, 12, "multi-line command should count all lines");
    }

    #[test]
    fn test_multi_line_diff() {
        let diff = "+fn foo() {{\n+    bar();\n+}}\n-fn old() {{\n-    // nothing\n}}";
        let req = ApprovalRequest::FileEdit {
            file_path: "src/lib.rs".into(),
            diff: diff.into(),
        };
        let overlay = ApprovalOverlay::new(req);
        // 2 header + 6 diff lines + 4 = 12
        let h = overlay.desired_height(80);
        assert_eq!(h, 12, "multi-line diff should count all diff lines");
    }

    #[test]
    fn test_huge_json_args() {
        let mut obj = serde_json::Map::new();
        for i in 0..50 {
            obj.insert(format!("key_{i}"), serde_json::Value::String("value".into()));
        }
        let req = ApprovalRequest::ToolCall {
            tool_name: "bash".into(),
            args: serde_json::Value::Object(obj),
        };
        let overlay = ApprovalOverlay::new(req);
        // Should not panic and should be capped at 24
        let h = overlay.desired_height(80);
        assert_eq!(h, 24, "huge JSON args should be capped at 24");
    }

    // -----------------------------------------------------------------------
    // Error paths — invalid input, overflow, panic scenarios
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_error_empty_string() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        overlay.set_error("".into());
        assert_eq!(overlay.error.as_deref(), Some(""));
    }

    #[test]
    fn test_set_error_after_result() {
        let req = ApprovalRequest::Command {
            command: "rm".into(),
            description: "Remove".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        overlay.handle_key_event(key(KeyCode::Char('y')));
        assert_eq!(overlay.result(), Some(ApprovalResult::Allow));
        // Set error after decision — should still work
        overlay.set_error("Something went wrong".into());
        assert_eq!(overlay.error.as_deref(), Some("Something went wrong"));
        assert_eq!(overlay.result(), Some(ApprovalResult::Allow));
        assert!(overlay.is_complete());
    }

    #[test]
    fn test_render_zero_area_no_panic() {
        // Create overlay for each variant and render with zero Rect
        let cmd_req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let file_req = ApprovalRequest::FileEdit {
            file_path: "f.rs".into(),
            diff: "+a\n-b".into(),
        };
        let tool_req = ApprovalRequest::ToolCall {
            tool_name: "bash".into(),
            args: serde_json::json!({"cmd": "echo"}),
        };

        for req in [cmd_req, file_req, tool_req] {
            let mut overlay = ApprovalOverlay::new(req);
            let mut buf = Buffer::empty(Rect::new(0, 0, 0, 0));
            overlay.render(Rect::new(0, 0, 0, 0), &mut buf);
            // Toggle show_diff and render again
            overlay.handle_key_event(key(KeyCode::Char('d')));
            let mut buf2 = Buffer::empty(Rect::new(0, 0, 0, 0));
            overlay.render(Rect::new(0, 0, 0, 0), &mut buf2);
        }
    }

    #[test]
    fn test_render_with_error_no_panic() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        overlay.set_error("Error: command not found".into());
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
        overlay.render(Rect::new(0, 0, 80, 24), &mut buf);
    }

    #[test]
    fn test_render_all_variants_no_panic() {
        let variants: Vec<ApprovalRequest> = vec![
            ApprovalRequest::Command {
                command: "cargo build".into(),
                description: "Build the project in release mode".into(),
            },
            ApprovalRequest::FileEdit {
                file_path: "src/main.rs".into(),
                diff: "+fn main() {{\n+    println!(\"hello\");\n+}}".into(),
            },
            ApprovalRequest::ToolCall {
                tool_name: "bash".into(),
                args: serde_json::json!({"command": "echo hi", "timeout": 30}),
            },
        ];

        for req in variants {
            let overlay = ApprovalOverlay::new(req);
            let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
            overlay.render(Rect::new(0, 0, 80, 24), &mut buf);
        }
    }

    // -----------------------------------------------------------------------
    // State transitions — result persistence, toggle interactions
    // -----------------------------------------------------------------------

    #[test]
    fn test_transition_y_then_d() {
        // Press Y (Allow) then D (toggle): result stays Allow, show_diff toggles
        let req = ApprovalRequest::FileEdit {
            file_path: "test.rs".into(),
            diff: "+a\n-b".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        overlay.handle_key_event(key(KeyCode::Char('y')));
        assert_eq!(overlay.result(), Some(ApprovalResult::Allow));
        assert!(overlay.is_complete());
        assert!(!overlay.show_diff());

        // After Y, pressing D should still toggle show_diff
        overlay.handle_key_event(key(KeyCode::Char('d')));
        assert_eq!(overlay.result(), Some(ApprovalResult::Allow)); // unchanged
        assert!(overlay.show_diff()); // toggled on
        assert!(overlay.is_complete()); // still complete

        // Press D again to toggle off
        overlay.handle_key_event(key(KeyCode::Char('d')));
        assert_eq!(overlay.result(), Some(ApprovalResult::Allow));
        assert!(!overlay.show_diff()); // toggled off
    }

    #[test]
    fn test_transition_d_then_y() {
        // Press D (toggle) then Y (Allow): result = Allow, show_diff stays true
        let req = ApprovalRequest::FileEdit {
            file_path: "test.rs".into(),
            diff: "+a\n-b".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        overlay.handle_key_event(key(KeyCode::Char('d')));
        assert!(!overlay.is_complete()); // D doesn't complete
        assert!(overlay.show_diff()); // toggled on

        overlay.handle_key_event(key(KeyCode::Char('y')));
        assert_eq!(overlay.result(), Some(ApprovalResult::Allow));
        assert!(overlay.is_complete());
        // show_diff should remain true (D was pressed before Y)
        assert!(overlay.show_diff());
    }

    #[test]
    fn test_transition_esc_then_y() {
        // Press Esc (Deny) then Y (Allow): result changes to Allow
        let req = ApprovalRequest::ToolCall {
            tool_name: "bash".into(),
            args: serde_json::json!({"cmd": "rm -rf /"}),
        };
        let mut overlay = ApprovalOverlay::new(req);
        overlay.handle_key_event(key(KeyCode::Esc));
        assert_eq!(overlay.result(), Some(ApprovalResult::Deny));
        assert!(overlay.is_complete());

        // After Esc, pressing Y should overwrite result to Allow
        overlay.handle_key_event(key(KeyCode::Char('y')));
        assert_eq!(overlay.result(), Some(ApprovalResult::Allow));
        assert!(overlay.is_complete());
    }

    #[test]
    fn test_transition_n_then_a() {
        // Press N (Deny) then A (AlwaysAllow): result changes
        let req = ApprovalRequest::Command {
            command: "cargo build".into(),
            description: "Build".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        overlay.handle_key_event(key(KeyCode::Char('n')));
        assert_eq!(overlay.result(), Some(ApprovalResult::Deny));

        overlay.handle_key_event(key(KeyCode::Char('a')));
        assert_eq!(overlay.result(), Some(ApprovalResult::AlwaysAllow));
    }

    #[test]
    fn test_transition_after_ctrl_c() {
        // Ctrl+C sets Deny, then pressing Y should change to Allow
        let req = ApprovalRequest::Command {
            command: "danger".into(),
            description: "Dangerous command".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        overlay.on_ctrl_c();
        assert_eq!(overlay.result(), Some(ApprovalResult::Deny));
        assert!(overlay.is_complete());

        // After Ctrl+C, pressing Y changes result
        overlay.handle_key_event(key(KeyCode::Char('y')));
        assert_eq!(overlay.result(), Some(ApprovalResult::Allow));
    }

    #[test]
    fn test_multiple_d_toggles() {
        // D pressed 3 times: on → off → on
        let req = ApprovalRequest::FileEdit {
            file_path: "f.rs".into(),
            diff: "+a".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        assert!(!overlay.show_diff());

        overlay.handle_key_event(key(KeyCode::Char('d')));
        assert!(overlay.show_diff());
        assert!(!overlay.is_complete());

        overlay.handle_key_event(key(KeyCode::Char('d')));
        assert!(!overlay.show_diff());
        assert!(!overlay.is_complete());

        overlay.handle_key_event(key(KeyCode::Char('d')));
        assert!(overlay.show_diff());
        assert!(!overlay.is_complete());
    }

    #[test]
    fn test_state_remains_complete_after_extra_key() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        overlay.handle_key_event(key(KeyCode::Char('y')));
        assert!(overlay.is_complete());

        // Pressing another key should complete still be true
        overlay.handle_key_event(key(KeyCode::Char('z')));
        assert!(overlay.is_complete(), "is_complete should remain true");

        overlay.handle_key_event(key(KeyCode::Char('d')));
        assert!(overlay.is_complete(), "is_complete should remain true even after D");
    }

    #[test]
    fn test_d_toggle_preserves_none_result() {
        // D toggle should not set result
        let req = ApprovalRequest::FileEdit {
            file_path: "f.rs".into(),
            diff: "+a".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        assert!(overlay.result().is_none());
        overlay.handle_key_event(key(KeyCode::Char('d')));
        assert!(overlay.result().is_none(), "D toggle should not set result");
        assert!(!overlay.is_complete(), "D toggle should not complete");
    }

    #[test]
    fn test_all_variants_initial_state() {
        let variants: Vec<ApprovalRequest> = vec![
            ApprovalRequest::Command {
                command: "ls".into(),
                description: "List".into(),
            },
            ApprovalRequest::FileEdit {
                file_path: "f.rs".into(),
                diff: "+a".into(),
            },
            ApprovalRequest::ToolCall {
                tool_name: "bash".into(),
                args: serde_json::json!({"a": 1}),
            },
        ];

        for req in variants {
            let overlay = ApprovalOverlay::new(req);
            assert!(overlay.result().is_none());
            assert!(!overlay.show_diff());
            assert!(!overlay.is_complete());
            assert_eq!(overlay.view_id(), Some("approval"));
        }
    }

    // -----------------------------------------------------------------------
    // Key event edge cases — modifiers, repeat, release
    // -----------------------------------------------------------------------

    #[test]
    fn test_key_with_shift_modifier() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);

        // Shift+Y should still be treated as 'y' after to_ascii_lowercase
        let shift_y = KeyEvent {
            code: KeyCode::Char('Y'),
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        assert_eq!(
            overlay.handle_key_event(shift_y),
            Handled::Handled
        );
        assert_eq!(overlay.result(), Some(ApprovalResult::Allow));
    }

    #[test]
    fn test_key_repeat_kind_handled() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);

        // Even with Repeat kind, the key should be handled
        let repeat_y = KeyEvent {
            code: KeyCode::Char('y'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Repeat,
            state: KeyEventState::NONE,
        };
        assert_eq!(
            overlay.handle_key_event(repeat_y),
            Handled::Handled
        );
        assert_eq!(overlay.result(), Some(ApprovalResult::Allow));
    }

    #[test]
    fn test_key_release_kind_handled() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);

        // Even with Release kind, the key should be handled
        let release_n = KeyEvent {
            code: KeyCode::Char('n'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        assert_eq!(
            overlay.handle_key_event(release_n),
            Handled::Handled
        );
        assert_eq!(overlay.result(), Some(ApprovalResult::Deny));
    }

    // -----------------------------------------------------------------------
    // Renderable trait — cursor methods for all variants
    // -----------------------------------------------------------------------

    #[test]
    fn test_cursor_all_variants_return_none() {
        let variants: Vec<ApprovalRequest> = vec![
            ApprovalRequest::Command {
                command: "ls".into(),
                description: "List".into(),
            },
            ApprovalRequest::FileEdit {
                file_path: "f.rs".into(),
                diff: "+a".into(),
            },
            ApprovalRequest::ToolCall {
                tool_name: "bash".into(),
                args: serde_json::json!({"a": 1}),
            },
        ];

        let area = Rect::new(0, 0, 80, 24);
        for req in variants {
            let overlay = ApprovalOverlay::new(req);
            assert_eq!(overlay.cursor_pos(area), None);
            assert_eq!(
                overlay.cursor_style(area),
                SetCursorStyle::DefaultUserShape
            );
        }
    }

    #[test]
    fn test_cursor_zero_area() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let overlay = ApprovalOverlay::new(req);
        let area = Rect::new(0, 0, 0, 0);
        assert_eq!(overlay.cursor_pos(area), None);
        assert_eq!(
            overlay.cursor_style(area),
            SetCursorStyle::DefaultUserShape
        );
    }

    // -----------------------------------------------------------------------
    // PaneView trait — completeness and view_id invariants
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_complete_after_all_decision_keys() {
        let decision_keys = ['y', 'Y', 'n', 'N', 'a', 'A'];
        for c in decision_keys {
            let req = ApprovalRequest::Command {
                command: "ls".into(),
                description: "List".into(),
            };
            let mut overlay = ApprovalOverlay::new(req);
            overlay.handle_key_event(key(KeyCode::Char(c)));
            assert!(
                overlay.is_complete(),
                "key '{c}' should make overlay complete"
            );
        }
    }

    #[test]
    fn test_is_complete_after_esc() {
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        overlay.handle_key_event(key(KeyCode::Esc));
        assert!(overlay.is_complete(), "Esc should make overlay complete");
    }

    #[test]
    fn test_is_complete_false_for_non_decision_keys() {
        let non_decision = ['d', 'D', 'z', 'x', ' ', '\t'];
        for c in non_decision {
            let req = ApprovalRequest::Command {
                command: "ls".into(),
                description: "List".into(),
            };
            let mut overlay = ApprovalOverlay::new(req);
            overlay.handle_key_event(key(KeyCode::Char(c)));
            assert!(
                !overlay.is_complete(),
                "key '{c}' should NOT make overlay complete"
            );
        }
    }

    #[test]
    fn test_view_id_consistent() {
        let variants: Vec<ApprovalRequest> = vec![
            ApprovalRequest::Command {
                command: "a".into(),
                description: "b".into(),
            },
            ApprovalRequest::FileEdit {
                file_path: "c".into(),
                diff: "d".into(),
            },
            ApprovalRequest::ToolCall {
                tool_name: "e".into(),
                args: serde_json::json!(null),
            },
        ];
        for req in variants {
            let overlay = ApprovalOverlay::new(req);
            assert_eq!(overlay.view_id(), Some("approval"));
        }
    }

    // -----------------------------------------------------------------------
    // desired_height — precise calculations
    // -----------------------------------------------------------------------

    #[test]
    fn test_desired_height_command_precise() {
        let req = ApprovalRequest::Command {
            command: "echo hello world".into(),
            description: "Print a greeting\nThis is a multi-line description".into(),
        };
        let overlay = ApprovalOverlay::new(req);
        // 2 desc lines + 1 cmd line + 2 labels + 4 = 9
        assert_eq!(overlay.desired_height(80), 9);
    }

    #[test]
    fn test_desired_height_file_edit_precise() {
        let req = ApprovalRequest::FileEdit {
            file_path: "src/main.rs".into(),
            diff: "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,5 @@\n+fn new() {{}}\n".into(),
        };
        let overlay = ApprovalOverlay::new(req);
        // 2 header + 4 diff lines + 4 = 10
        assert_eq!(overlay.desired_height(80), 10);
    }

    #[test]
    fn test_desired_height_tool_call_precise() {
        let req = ApprovalRequest::ToolCall {
            tool_name: "read_file".into(),
            args: serde_json::json!({"path": "/etc/hosts"}),
        };
        let overlay = ApprovalOverlay::new(req);
        // pretty-printed JSON = 3 lines, 2 + 3 + 4 = 9
        assert_eq!(overlay.desired_height(80), 9);
    }

    // -----------------------------------------------------------------------
    // Render does not panic with show_diff on all variants
    // -----------------------------------------------------------------------

    #[test]
    fn test_render_with_show_diff_command_no_panic() {
        // show_diff doesn't affect Command rendering, but should not panic
        let req = ApprovalRequest::Command {
            command: "ls".into(),
            description: "List".into(),
        };
        let mut overlay = ApprovalOverlay::new(req);
        overlay.handle_key_event(key(KeyCode::Char('d')));
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
        overlay.render(Rect::new(0, 0, 80, 24), &mut buf);
    }

    #[test]
    fn test_render_with_show_diff_tool_call_no_panic() {
        let req = ApprovalRequest::ToolCall {
            tool_name: "bash".into(),
            args: serde_json::json!({"cmd": "echo"}),
        };
        let mut overlay = ApprovalOverlay::new(req);
        overlay.handle_key_event(key(KeyCode::Char('d')));
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
        overlay.render(Rect::new(0, 0, 80, 24), &mut buf);
    }
}