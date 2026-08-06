//! File diff rendering component.
//!
//! Parses unified diff format and renders it with color-coded line types:
//!
//! - Added lines (`+`) in green
//! - Removed lines (`-`) in red
//! - Header/metadata lines (`@@`, `diff --git`, `---`/`+++`, `index`, etc.) in blue
//! - Context lines in default terminal style
//!
//! # Quick Start
//!
//! ```ignore
//! let diff = DiffView::parse_diff(diff_text);
//! diff.render(area, buf);
//! ```
//!
//! For a bordered preview with a file-path title, use [`DiffPreview`].

use crate::tui::render::Renderable;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::Widget,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders},
};

// ---------------------------------------------------------------------------
// DiffLineType
// ---------------------------------------------------------------------------

/// The type of a single line in a unified diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineType {
    /// Unchanged context line (prefixed with a space in the raw diff).
    Context,
    /// Added line (prefixed with `+` in the raw diff).
    Added,
    /// Removed line (prefixed with `-` in the raw diff).
    Removed,
    /// Header or metadata line (`diff --git`, `---`, `+++`, `@@`, `index`, etc.).
    Header,
}

// ---------------------------------------------------------------------------
// DiffLine
// ---------------------------------------------------------------------------

/// A single parsed line in a unified diff.
///
/// The `content` field stores the line **without** the leading `+`/`-`/` `
/// prefix that is present in the raw unified diff format.  Header lines store
/// their full text (including any `@@` markers, `diff --git` prefix, etc.).
#[derive(Debug, Clone)]
pub struct DiffLine {
    /// The type/category of this line.
    pub line_type: DiffLineType,
    /// The line content (stripped of the unified-diff prefix character).
    pub content: String,
}

// ---------------------------------------------------------------------------
// DiffView
// ---------------------------------------------------------------------------

/// A renderable diff view that displays a unified diff with colour-coded lines.
///
/// Parses standard unified diff output (e.g. from `git diff`) and renders each
/// line according to its type:
///
/// | Type    | Colour  | Prefix shown |
/// |---------|---------|--------------|
/// | Added   | Green   | `+`          |
/// | Removed | Red     | `-`          |
/// | Header  | Blue    | *(none)*     |
/// | Context | Default | ` ` (space)  |
pub struct DiffView {
    /// Parsed diff lines in order.
    pub lines: Vec<DiffLine>,
    /// The file path being diffed (extracted from `+++` or `diff --git`).
    pub file_path: String,
}

impl DiffView {
    /// Parse a unified diff string into a `DiffView`.
    ///
    /// Handles standard unified diff format produced by `git diff`, `diff -u`,
    /// and similar tools:
    ///
    /// - `diff --git a/path b/path` — header
    /// - `--- a/path` — header (old file)
    /// - `+++ b/path` — header (new file); also sets `file_path`
    /// - `@@ -a,b +c,d @@ ...` — hunk header
    /// - `index abc..def mode` — header
    /// - `new file mode` / `deleted file mode` — header
    /// - `+content` — added line
    /// - `-content` — removed line
    /// - ` content` — context line
    /// - `\\ No newline at end of file` — header
    pub fn parse_diff(input: &str) -> Self {
        let mut lines: Vec<DiffLine> = Vec::new();
        let mut file_path = String::new();

        for raw_line in input.lines() {
            let (line_type, content) = classify_line(raw_line);

            // Extract file path from the `+++ b/...` line.
            if line_type == DiffLineType::Header && raw_line.starts_with("+++") {
                let trimmed = raw_line.get(4..).map_or("", |s| s.trim());
                if let Some(stripped) = trimmed.strip_prefix("b/") {
                    file_path = stripped.to_string();
                } else {
                    file_path = trimmed.to_string();
                }
            }

            lines.push(DiffLine { line_type, content });
        }

        // Fallback: try `diff --git a/path b/path` if no `+++` was found.
        if file_path.is_empty() {
            for raw_line in input.lines() {
                if let Some(b_path) = raw_line.strip_prefix("diff --git ") {
                    if let Some((_, b)) = b_path.rsplit_once(" b/") {
                        file_path = b.to_string();
                        break;
                    }
                }
            }
        }

        Self { lines, file_path }
    }

    /// Return the style for a given line type.
    fn style_for(line_type: DiffLineType) -> Style {
        match line_type {
            DiffLineType::Added => Style::default().fg(Color::Green),
            DiffLineType::Removed => Style::default().fg(Color::Red),
            DiffLineType::Header => {
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD)
            }
            DiffLineType::Context => Style::default(),
        }
    }

    /// Return the prefix character to display for a given line type.
    fn prefix_for(line_type: DiffLineType) -> &'static str {
        match line_type {
            DiffLineType::Added => "+",
            DiffLineType::Removed => "-",
            DiffLineType::Header => "",
            DiffLineType::Context => " ",
        }
    }
}

impl Renderable for DiffView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if self.lines.is_empty() {
            return;
        }

        let max_y = area.y + area.height;
        let mut y = area.y;

        for line in &self.lines {
            if y >= max_y {
                break;
            }

            let style = Self::style_for(line.line_type);
            let prefix = Self::prefix_for(line.line_type);

            let spans = if prefix.is_empty() {
                // Header lines: full content in styled form.
                vec![Span::styled(&line.content, style)]
            } else {
                // Added / Removed / Context: style prefix + content.
                vec![
                    Span::styled(prefix, style),
                    Span::styled(&line.content, style),
                ]
            };

            let line_area = Rect::new(area.x, y, area.width, 1);
            Line::from(spans).render(line_area, buf);

            y += 1;
        }
    }

    fn desired_height(&self, _width: u16) -> u16 {
        self.lines.len() as u16
    }
}

// ---------------------------------------------------------------------------
// DiffPreview — bordered wrapper
// ---------------------------------------------------------------------------

/// A diff preview component with a bordered block and a file-path title.
///
/// Wraps a [`DiffView`] inside a bordered block whose title shows the file
/// path.  The inner area is inset by one cell on each side.
///
/// # Example
///
/// ```ignore
/// let preview = DiffPreview::new(diff_text);
/// preview.render(area, buf);
/// ```
pub struct DiffPreview {
    /// The underlying parsed diff view.
    diff_view: DiffView,
}

impl DiffPreview {
    /// Create a new `DiffPreview` from a unified diff string.
    pub fn new(diff_text: &str) -> Self {
        Self {
            diff_view: DiffView::parse_diff(diff_text),
        }
    }

    /// Access the underlying parsed diff view.
    pub fn diff_view(&self) -> &DiffView {
        &self.diff_view
    }
}

impl Renderable for DiffPreview {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let title = if self.diff_view.file_path.is_empty() {
            " 差异 ".to_string()
        } else {
            format!(" 差异: {} ", self.diff_view.file_path)
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue));
        block.render(area, buf);

        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        self.diff_view.render(inner, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        let line_count = self.diff_view.lines.len() as u16;
        line_count + 2 // top and bottom borders
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Classify a single raw line from unified diff output and return its type
/// together with the content stripped of the leading diff-prefix character.
fn classify_line(raw: &str) -> (DiffLineType, String) {
    if raw.is_empty() {
        return (DiffLineType::Context, String::new());
    }

    let first = raw.as_bytes()[0] as char;

    match first {
        '+' => {
            if raw.starts_with("+++") {
                (DiffLineType::Header, raw.to_string())
            } else {
                (DiffLineType::Added, raw[1..].to_string())
            }
        }
        '-' => {
            if raw.starts_with("---") {
                (DiffLineType::Header, raw.to_string())
            } else {
                (DiffLineType::Removed, raw[1..].to_string())
            }
        }
        ' ' => (DiffLineType::Context, raw[1..].to_string()),
        '@' if raw.starts_with("@@") => {
            (DiffLineType::Header, raw.to_string())
        }
        _ => {
            // Treat everything else (diff --git, ---, +++, index, mode,
            // "\\ No newline", etc.) as a header line.
            (DiffLineType::Header, raw.to_string())
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

    // -----------------------------------------------------------------------
    // DiffLineType & classify_line
    // -----------------------------------------------------------------------

    #[test]
    fn test_classify_added() {
        let (ty, content) = classify_line("+fn new()");
        assert_eq!(ty, DiffLineType::Added);
        assert_eq!(content, "fn new()");
    }

    #[test]
    fn test_classify_removed() {
        let (ty, content) = classify_line("-fn old()");
        assert_eq!(ty, DiffLineType::Removed);
        assert_eq!(content, "fn old()");
    }

    #[test]
    fn test_classify_context() {
        let (ty, content) = classify_line(" fn existing()");
        assert_eq!(ty, DiffLineType::Context);
        assert_eq!(content, "fn existing()");
    }

    #[test]
    fn test_classify_header_hunk() {
        let (ty, content) = classify_line("@@ -1,3 +1,4 @@");
        assert_eq!(ty, DiffLineType::Header);
        assert_eq!(content, "@@ -1,3 +1,4 @@");
    }

    #[test]
    fn test_classify_header_diff_git() {
        let (ty, content) = classify_line("diff --git a/src/main.rs b/src/main.rs");
        assert_eq!(ty, DiffLineType::Header);
        assert_eq!(content, "diff --git a/src/main.rs b/src/main.rs");
    }

    #[test]
    fn test_classify_empty_line() {
        let (ty, content) = classify_line("");
        assert_eq!(ty, DiffLineType::Context);
        assert_eq!(content, "");
    }

    // -----------------------------------------------------------------------
    // DiffView::parse_diff
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_diff_basic() {
        let input = "\
diff --git a/src/main.rs b/src/main.rs
index abc123..def456 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
-    println!(\"hello\");
+    println!(\"hello world\");
+    println!(\"goodbye\");
 }
";
        let diff = DiffView::parse_diff(input);
        assert_eq!(diff.file_path, "src/main.rs");
        assert_eq!(diff.lines.len(), 10);

        // Types in order
        let types: Vec<DiffLineType> = diff.lines.iter().map(|l| l.line_type).collect();
        assert_eq!(
            types,
            vec![
                DiffLineType::Header, // diff --git
                DiffLineType::Header, // index
                DiffLineType::Header, // ---
                DiffLineType::Header, // +++
                DiffLineType::Header, // @@
                DiffLineType::Context, // fn main() {
                DiffLineType::Removed, // -println!("hello");
                DiffLineType::Added,   // +println!("hello world");
                DiffLineType::Added,   // +println!("goodbye");
                DiffLineType::Context, // }
            ]
        );
    }

    #[test]
    fn test_parse_diff_empty() {
        let diff = DiffView::parse_diff("");
        assert!(diff.file_path.is_empty());
        assert!(diff.lines.is_empty());
    }

    #[test]
    fn test_parse_diff_no_path() {
        let input = "\
@@ -1 +1 @@
-old
+new
";
        let diff = DiffView::parse_diff(input);
        assert!(diff.file_path.is_empty());
        assert_eq!(diff.lines.len(), 3);
    }

    // -----------------------------------------------------------------------
    // DiffView desired_height
    // -----------------------------------------------------------------------

    #[test]
    fn test_desired_height_empty() {
        let diff = DiffView::parse_diff("");
        assert_eq!(diff.desired_height(80), 0);
    }

    #[test]
    fn test_desired_height_single_line() {
        let diff = DiffView::parse_diff("+new line\n");
        assert_eq!(diff.desired_height(80), 1);
    }

    #[test]
    fn test_desired_height_multi_line() {
        let diff = DiffView::parse_diff("+a\n-b\n c\n@@ -1 +1 @@\n");
        assert_eq!(diff.desired_height(80), 4);
    }

    // -----------------------------------------------------------------------
    // DiffView render (smoke-test — no crash, correct content)
    // -----------------------------------------------------------------------

    #[test]
    fn test_render_empty() {
        let diff = DiffView::parse_diff("");
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        diff.render(area, &mut buf);
        // No crash, all cells are blank.
    }

    #[test]
    fn test_render_added_line() {
        let diff = DiffView::parse_diff("+hello\n");
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        diff.render(area, &mut buf);

        // Cell (0,0) should show '+'
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("+"));
        // Cell (1,0) should show 'h'
        assert_eq!(buf.cell((1, 0)).map(|c| c.symbol()), Some("h"));
        // Style should be green
        let style = buf.cell((0, 0)).map(|c| c.style());
        assert_eq!(style, Some(Style::default().fg(Color::Green).bg(Color::Reset).underline_color(Color::Reset)));
    }

    #[test]
    fn test_render_removed_line() {
        let diff = DiffView::parse_diff("-bye\n");
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        diff.render(area, &mut buf);

        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("-"));
        assert_eq!(buf.cell((1, 0)).map(|c| c.symbol()), Some("b"));
        let style = buf.cell((0, 0)).map(|c| c.style());
        assert_eq!(style, Some(Style::default().fg(Color::Red).bg(Color::Reset).underline_color(Color::Reset)));
    }

    #[test]
    fn test_render_header_line() {
        let diff = DiffView::parse_diff("@@ -1 +1 @@\n");
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        diff.render(area, &mut buf);

        // Header lines have no prefix; the content starts at column 0.
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("@"));
        assert_eq!(buf.cell((1, 0)).map(|c| c.symbol()), Some("@"));
        let style = buf.cell((0, 0)).map(|c| c.style());
        assert_eq!(
            style,
            Some(Style::default().fg(Color::Blue).bg(Color::Reset).underline_color(Color::Reset).add_modifier(Modifier::BOLD))
        );
    }

    #[test]
    fn test_render_clips_to_area() {
        let diff = DiffView::parse_diff("+a\n+b\n+c\n+d\n");
        let area = Rect::new(0, 0, 80, 2); // only 2 rows
        let mut buf = Buffer::empty(area);
        diff.render(area, &mut buf);

        // First two lines rendered
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("+"));
        assert_eq!(buf.cell((0, 1)).map(|c| c.symbol()), Some("+"));
        // Third line clipped
        assert_eq!(buf.cell((0, 2)), None);
    }

    // -----------------------------------------------------------------------
    // DiffPreview
    // -----------------------------------------------------------------------

    #[test]
    fn test_diff_preview_desired_height() {
        let preview = DiffPreview::new("+a\n+b\n");
        // 2 lines + 2 borders
        assert_eq!(preview.desired_height(80), 4);
    }

    #[test]
    fn test_diff_preview_empty() {
        let preview = DiffPreview::new("");
        // 0 lines + 2 borders
        assert_eq!(preview.desired_height(80), 2);
    }

    #[test]
    fn test_diff_preview_borrows_view() {
        let preview = DiffPreview::new("+a\n");
        assert_eq!(preview.diff_view().lines.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Renderable trait defaults for DiffView
    // -----------------------------------------------------------------------

    #[test]
    fn test_diff_view_cursor_pos_default() {
        let diff = DiffView::parse_diff("+a\n");
        let area = Rect::new(0, 0, 80, 10);
        assert_eq!(diff.cursor_pos(area), None);
    }

    #[test]
    fn test_diff_view_cursor_style_default() {
        let diff = DiffView::parse_diff("+a\n");
        let area = Rect::new(0, 0, 80, 10);
        assert_eq!(diff.cursor_style(area), SetCursorStyle::DefaultUserShape);
    }

    // -----------------------------------------------------------------------
    // Renderable trait defaults for DiffPreview
    // -----------------------------------------------------------------------

    #[test]
    fn test_diff_preview_cursor_pos_default() {
        let preview = DiffPreview::new("+a\n");
        let area = Rect::new(0, 0, 80, 10);
        assert_eq!(preview.cursor_pos(area), None);
    }

    #[test]
    fn test_diff_preview_cursor_style_default() {
        let preview = DiffPreview::new("+a\n");
        let area = Rect::new(0, 0, 80, 10);
        assert_eq!(preview.cursor_style(area), SetCursorStyle::DefaultUserShape);
    }

    // -----------------------------------------------------------------------
    // classify_line — edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_classify_triple_plus_is_header() {
        let (ty, content) = classify_line("+++ b/src/main.rs");
        assert_eq!(ty, DiffLineType::Header);
        assert_eq!(content, "+++ b/src/main.rs");
    }

    #[test]
    fn test_classify_triple_dash_is_header() {
        let (ty, content) = classify_line("--- a/src/main.rs");
        assert_eq!(ty, DiffLineType::Header);
        assert_eq!(content, "--- a/src/main.rs");
    }

    #[test]
    fn test_classify_index_line() {
        let (ty, content) = classify_line("index abc123..def456 100644");
        assert_eq!(ty, DiffLineType::Header);
        assert_eq!(content, "index abc123..def456 100644");
    }

    #[test]
    fn test_classify_new_file_mode() {
        let (ty, _) = classify_line("new file mode 100644");
        assert_eq!(ty, DiffLineType::Header);
    }

    #[test]
    fn test_classify_deleted_file_mode() {
        let (ty, _) = classify_line("deleted file mode 100644");
        assert_eq!(ty, DiffLineType::Header);
    }

    #[test]
    fn test_classify_no_newline() {
        let (ty, content) = classify_line("\\ No newline at end of file");
        assert_eq!(ty, DiffLineType::Header);
        assert_eq!(content, "\\ No newline at end of file");
    }

    #[test]
    fn test_classify_bare_plus() {
        let (ty, content) = classify_line("+");
        assert_eq!(ty, DiffLineType::Added);
        assert_eq!(content, "");
    }

    #[test]
    fn test_classify_bare_minus() {
        let (ty, content) = classify_line("-");
        assert_eq!(ty, DiffLineType::Removed);
        assert_eq!(content, "");
    }

    #[test]
    fn test_classify_bare_space() {
        let (ty, content) = classify_line(" ");
        assert_eq!(ty, DiffLineType::Context);
        assert_eq!(content, "");
    }

    // -----------------------------------------------------------------------
    // parse_diff — boundary and edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_diff_fallback_path_from_diff_git() {
        // No +++ line, but has diff --git — fallback should extract path.
        let input = "\
diff --git a/src/lib.rs b/src/lib.rs
index abc..def 100644
@@ -1 +1 @@
-old
+new
";
        let diff = DiffView::parse_diff(input);
        assert_eq!(diff.file_path, "src/lib.rs");
        assert_eq!(diff.lines.len(), 5);
    }

    #[test]
    fn test_parse_diff_plusplus_without_b_prefix() {
        // +++ without b/ prefix — should use the trimmed text directly.
        let input = "+++ /dev/null\n";
        let diff = DiffView::parse_diff(input);
        assert_eq!(diff.file_path, "/dev/null");
    }

    #[test]
    fn test_parse_diff_whitespace_only_lines() {
        let input = "+  \n-  \n   \n";
        let diff = DiffView::parse_diff(input);
        assert_eq!(diff.lines.len(), 3);
        assert_eq!(diff.lines[0].line_type, DiffLineType::Added);
        assert_eq!(diff.lines[0].content, "  ");
        assert_eq!(diff.lines[1].line_type, DiffLineType::Removed);
        assert_eq!(diff.lines[1].content, "  ");
        assert_eq!(diff.lines[2].line_type, DiffLineType::Context);
        assert_eq!(diff.lines[2].content, "  ");
    }

    #[test]
    fn test_parse_diff_new_file_mode() {
        let input = "diff --git a/new.txt b/new.txt\nnew file mode 100644\n";
        let diff = DiffView::parse_diff(input);
        assert_eq!(diff.file_path, "new.txt");
        assert_eq!(diff.lines.len(), 2);
        assert_eq!(diff.lines[1].line_type, DiffLineType::Header);
    }

    #[test]
    fn test_parse_diff_deleted_file_mode() {
        let input = "diff --git a/old.txt b/old.txt\ndeleted file mode 100644\n";
        let diff = DiffView::parse_diff(input);
        assert_eq!(diff.file_path, "old.txt");
        assert_eq!(diff.lines.len(), 2);
        assert_eq!(diff.lines[1].line_type, DiffLineType::Header);
    }

    #[test]
    fn test_parse_diff_no_newline_at_eof() {
        let input = "\
@@ -1 +1 @@
-old
+new
\\ No newline at end of file
";
        let diff = DiffView::parse_diff(input);
        assert_eq!(diff.lines.len(), 4);
        assert_eq!(diff.lines[3].line_type, DiffLineType::Header);
        assert!(diff.lines[3].content.contains("No newline"));
    }

    #[test]
    fn test_parse_diff_large_input() {
        // Generate 300 lines to stress large input handling.
        let mut lines = Vec::new();
        lines.push("diff --git a/large.txt b/large.txt".to_string());
        lines.push("--- a/large.txt".to_string());
        lines.push("+++ b/large.txt".to_string());
        lines.push("@@ -1 +1 @@".to_string());
        for i in 0..300 {
            if i % 2 == 0 {
                lines.push(format!("+additional line {}", i));
            } else {
                lines.push(format!("-removed line {}", i));
            }
        }
        let input = lines.join("\n");
        let diff = DiffView::parse_diff(&input);
        // 3 header + 1 hunk + 300 content = 304
        assert_eq!(diff.lines.len(), 304);
        assert_eq!(diff.file_path, "large.txt");
        // desired_height should not overflow for 304 lines
        assert_eq!(diff.desired_height(80), 304);
    }

    #[test]
    fn test_parse_diff_plusplus_trumps_diff_git() {
        // When both +++ and diff --git exist, +++ should win.
        let input = "\
diff --git a/other.txt b/other.txt
+++ b/src/main.rs
";
        let diff = DiffView::parse_diff(input);
        assert_eq!(diff.file_path, "src/main.rs");
    }

    // -----------------------------------------------------------------------
    // render — boundary conditions
    // -----------------------------------------------------------------------

    #[test]
    fn test_render_zero_height_area() {
        let diff = DiffView::parse_diff("+a\n+b\n");
        let area = Rect::new(0, 0, 80, 0);
        let mut buf = Buffer::empty(area);
        diff.render(area, &mut buf);
        // No crash, no cells rendered (area has 0 height).
    }

    #[test]
    fn test_render_zero_width_area() {
        let diff = DiffView::parse_diff("+a\n");
        let area = Rect::new(0, 0, 0, 10);
        let mut buf = Buffer::empty(area);
        diff.render(area, &mut buf);
        // No crash, Line renders within zero-width area.
    }

    #[test]
    fn test_render_at_offset_area() {
        let diff = DiffView::parse_diff("+hello\n");
        let area = Rect::new(5, 3, 80, 10);
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 20));
        diff.render(area, &mut buf);

        // Prefix '+' should be at (5, 3)
        assert_eq!(buf.cell((5, 3)).map(|c| c.symbol()), Some("+"));
        // Content 'h' at (6, 3)
        assert_eq!(buf.cell((6, 3)).map(|c| c.symbol()), Some("h"));
        // Area outside the target should be untouched (blank)
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some(" "));
    }

    #[test]
    fn test_render_context_line() {
        let diff = DiffView::parse_diff(" context\n");
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        diff.render(area, &mut buf);

        // Context line has space prefix at column 0
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some(" "));
        // Content starts at column 1
        assert_eq!(buf.cell((1, 0)).map(|c| c.symbol()), Some("c"));
        // Style should be default (no fg color override)
        let style = buf.cell((0, 0)).map(|c| c.style());
        assert_eq!(
            style,
            Some(Style::default().fg(Color::Reset).bg(Color::Reset).underline_color(Color::Reset))
        );
    }

    #[test]
    fn test_render_mixed_line_types() {
        let input = "\
@@ -1,3 +1,4 @@
 fn main() {
-    old_line
+    new_line
 }
";
        let diff = DiffView::parse_diff(input);
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        diff.render(area, &mut buf);

        // Line 0: header (no prefix, blue bold)
        assert_eq!(buf.cell((0, 0)).map(|c| c.symbol()), Some("@"));
        let hdr_style = buf.cell((0, 0)).map(|c| c.style());
        assert_eq!(
            hdr_style,
            Some(
                Style::default()
                    .fg(Color::Blue)
                    .bg(Color::Reset)
                    .underline_color(Color::Reset)
                    .add_modifier(Modifier::BOLD)
            )
        );

        // Line 1: context (space prefix, default style)
        assert_eq!(buf.cell((0, 1)).map(|c| c.symbol()), Some(" "));
        assert_eq!(buf.cell((1, 1)).map(|c| c.symbol()), Some("f"));

        // Line 2: removed (red)
        assert_eq!(buf.cell((0, 2)).map(|c| c.symbol()), Some("-"));
        let rem_style = buf.cell((0, 2)).map(|c| c.style());
        assert_eq!(
            rem_style,
            Some(
                Style::default()
                    .fg(Color::Red)
                    .bg(Color::Reset)
                    .underline_color(Color::Reset)
            )
        );

        // Line 3: added (green)
        assert_eq!(buf.cell((0, 3)).map(|c| c.symbol()), Some("+"));
        let add_style = buf.cell((0, 3)).map(|c| c.style());
        assert_eq!(
            add_style,
            Some(
                Style::default()
                    .fg(Color::Green)
                    .bg(Color::Reset)
                    .underline_color(Color::Reset)
            )
        );

        // Line 4: context (default)
        assert_eq!(buf.cell((0, 4)).map(|c| c.symbol()), Some(" "));
    }

    // -----------------------------------------------------------------------
    // DiffPreview — render tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_diff_preview_render_shows_border() {
        let preview = DiffPreview::new("+a\n");
        let area = Rect::new(0, 0, 20, 4);
        let mut buf = Buffer::empty(area);
        preview.render(area, &mut buf);

        // Border should be rendered — top-left corner is a box-drawing char
        let top_left = buf.cell((0, 0)).map(|c| c.symbol());
        assert!(top_left.is_some());
        // A border char is not a space
        assert_ne!(top_left.unwrap(), " ");
    }

    #[test]
    fn test_diff_preview_render_inner_content() {
        let preview = DiffPreview::new("+hello\n");
        let area = Rect::new(0, 0, 20, 4);
        let mut buf = Buffer::empty(area);
        preview.render(area, &mut buf);

        // Inner area starts at (1, 1) — the '+' should be at (1, 1)
        // (border occupies (0,0))
        assert_eq!(buf.cell((1, 1)).map(|c| c.symbol()), Some("+"));
        // Content 'h' at (2, 1)
        assert_eq!(buf.cell((2, 1)).map(|c| c.symbol()), Some("h"));
    }

    #[test]
    fn test_diff_preview_empty_file_path_title() {
        let preview = DiffPreview::new("");
        let area = Rect::new(0, 0, 10, 2);
        let mut buf = Buffer::empty(area);
        preview.render(area, &mut buf);

        // The title " 差异 " renders; verify no panic.
        let top_left = buf.cell((0, 0)).map(|c| c.symbol());
        assert!(top_left.is_some());
    }

    #[test]
    fn test_diff_preview_desired_height_large() {
        let mut lines = Vec::new();
        for i in 0..100 {
            lines.push(format!("+line {}", i));
        }
        let input = lines.join("\n");
        let preview = DiffPreview::new(&input);
        // 100 lines + 2 borders = 102
        assert_eq!(preview.desired_height(80), 102);
    }
}