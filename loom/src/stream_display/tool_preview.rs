//! Tool PREVIEW and DIFF rendering.
//!
//! Generates inline previews and diffs between CALL and DONE lines,
//! following the UX spec in docs/tool-display-ux.md.
//!
//! PREVIEW: Shows a snapshot of tool output (read, glob, grep, etc.)
//! DIFF: Shows edit changes with red/green highlighting

use crate::stream_display::panel_format::{color_enabled, format_panel_line};

/// Maximum number of preview lines before collapsing.
const MAX_PREVIEW_LINES: usize = 5;
/// Maximum number of glob files to show in preview.
const MAX_GLOB_FILES: usize = 10;
/// Maximum number of grep matches to show.
const MAX_GREP_MATCHES: usize = 5;

/// Generate a PREVIEW block for a tool, given its name, args JSON, and result text.
///
/// Returns None if the tool doesn't support preview or if in compact mode.
pub fn format_preview(tool_name: &str, args_json: &str, result: &str, compact: bool) -> Option<String> {
    if compact {
        return None;
    }

    match tool_name {
        "read" | "glob" | "grep" => {
            if result.trim().is_empty() {
                return None;
            }
        }
        "todo_write" | "todo_read" => {} // These don't need result text
        _ => return None,
    }

    match tool_name {
        "read" => Some(format_read_preview(args_json, result)),
        "glob" => Some(format_glob_preview(args_json, result)),
        "grep" => Some(format_grep_preview(args_json, result)),
        "todo_write" => Some(format_todo_write_preview(args_json)),
        "todo_read" => Some(format_todo_read_preview(result)),
        _ => None,
    }
}

/// Generate a DIFF block for edit/multiedit tools.
///
/// Returns None if the tool doesn't support diff.
pub fn format_diff(tool_name: &str, args_json: &str, _result: &str, compact: bool) -> Option<String> {
    if compact {
        return None;
    }

    match tool_name {
        "edit" => Some(format_edit_diff(args_json)),
        "multiedit" => Some(format_multiedit_diff(args_json)),
        _ => None,
    }
}

// ── PREVIEW formatters ───────────────────────────────────────────

fn format_read_preview(args_json: &str, result: &str) -> String {
    let args: serde_json::Value = serde_json::from_str(args_json).unwrap_or_default();
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
    let offset = args.get("offset").and_then(|v| v.as_u64());
    let limit = args.get("limit").and_then(|v| v.as_u64());

    let header = match (offset, limit) {
        (Some(o), Some(l)) => format!("{} [{}:{}]", path, o, o + l),
        (Some(o), None) => format!("{} [{}:]", path, o),
        (None, Some(l)) => format!("{} [:{}]", path, l),
        _ => path.to_string(),
    };

    let lines: Vec<&str> = result.lines().collect();
    let total = lines.len();
    let start_line = offset.unwrap_or(1) as usize;

    let mut output = format_panel_line("PREV", &header);
    output.push('\n');

    let show_lines = lines.iter().take(MAX_PREVIEW_LINES);
    let line_num_width = format!("{}", start_line + total.min(MAX_PREVIEW_LINES)).len();

    for (i, line) in show_lines.enumerate() {
        let line_num = start_line + i;
        let truncated = truncate_to_width(line, 100);
        output.push_str(&format_line_numbered(line_num, line_num_width, &truncated));
        output.push('\n');
    }

    if total > MAX_PREVIEW_LINES {
        let remaining = total - MAX_PREVIEW_LINES;
        output.push_str(&format_collapse_line(&format!("{} more lines", remaining)));
        output.push('\n');
    }

    output
}

fn format_glob_preview(args_json: &str, result: &str) -> String {
    let args: serde_json::Value = serde_json::from_str(args_json).unwrap_or_default();
    let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");

    let files: Vec<&str> = result.lines().filter(|l| !l.trim().is_empty()).collect();
    let total = files.len();

    let header = format!("{} ({} files)", pattern, total);
    let mut output = format_panel_line("PREV", &header);
    output.push('\n');

    for file in files.iter().take(MAX_GLOB_FILES) {
        let truncated = truncate_to_width(file, 100);
        output.push_str(&format!("       {}", truncated));
        output.push('\n');
    }

    if total > MAX_GLOB_FILES {
        let remaining = total - MAX_GLOB_FILES;
        output.push_str(&format_collapse_line(&format!("{} more files", remaining)));
        output.push('\n');
    }

    output
}

fn format_grep_preview(args_json: &str, result: &str) -> String {
    let args: serde_json::Value = serde_json::from_str(args_json).unwrap_or_default();
    let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");

    let matches: Vec<&str> = result.lines().filter(|l| !l.trim().is_empty()).collect();
    let total = matches.len();

    // Count unique files
    let files: std::collections::HashSet<&str> = matches
        .iter()
        .filter_map(|l| l.split(':').next())
        .collect();
    let file_count = files.len();

    let header = if file_count > 1 {
        format!("{} ({} matches in {} files)", pattern, total, file_count)
    } else {
        format!("{} ({} matches)", pattern, total)
    };

    let mut output = format_panel_line("PREV", &header);
    output.push('\n');

    for m in matches.iter().take(MAX_GREP_MATCHES) {
        let truncated = truncate_to_width(m, 100);
        output.push_str(&format!("       {}", truncated));
        output.push('\n');
    }

    if total > MAX_GREP_MATCHES {
        let remaining = total - MAX_GREP_MATCHES;
        output.push_str(&format_collapse_line(&format!("{} more matches", remaining)));
        output.push('\n');
    }

    output
}

fn format_todo_write_preview(args_json: &str) -> String {
    let args: serde_json::Value = serde_json::from_str(args_json).unwrap_or_default();
    let todos = match args.get("todos").and_then(|v| v.as_array()) {
        Some(t) => t,
        None => return String::new(),
    };

    let _total = todos.len();
    let header = format!("({} todos)", _total);
    let mut output = format_panel_line("PREV", &header);
    output.push('\n');

    for (i, todo) in todos.iter().enumerate() {
        let status = todo.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
        let content = todo.get("content").and_then(|v| v.as_str()).unwrap_or("?");
        let icon = match status {
            "completed" => "✓",
            "in_progress" => "●",
            "cancelled" => "⊘",
            _ => "○",
        };
        let truncated = truncate_to_width(content, 60);
        output.push_str(&format!(
            "       {} {}. {:<40} [{}]",
            icon,
            i + 1,
            truncated,
            status
        ));
        output.push('\n');
    }

    output
}

fn format_todo_read_preview(result: &str) -> String {
    // todo_read returns "{count} todos\n{json_array}"
    // Extract the JSON portion (everything after the first newline)
    let json_str = if let Some(pos) = result.find('\n') {
        &result[pos + 1..]
    } else {
        result
    };

    let todos: Vec<serde_json::Value> = if let Ok(v) = serde_json::from_str(json_str.trim()) {
        v
    } else {
        // Try parsing the whole result as JSON (backward compat)
        if let Ok(v) = serde_json::from_str(result.trim()) {
            v
        } else {
            return format_panel_line("PREV", result);
        }
    };

    let _total = todos.len();
    let pending = todos.iter().filter(|t| t.get("status").and_then(|v| v.as_str()) == Some("pending")).count();
    let in_progress = todos.iter().filter(|t| t.get("status").and_then(|v| v.as_str()) == Some("in_progress")).count();
    let completed = todos.iter().filter(|t| t.get("status").and_then(|v| v.as_str()) == Some("completed")).count();

    let header = format!("({} pending, {} in_progress, {} completed)", pending, in_progress, completed);
    let mut output = format_panel_line("PREV", &header);
    output.push('\n');

    for (i, todo) in todos.iter().enumerate() {
        let status = todo.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
        if status == "completed" || status == "cancelled" {
            continue;
        }
        let content = todo.get("content").and_then(|v| v.as_str()).unwrap_or("?");
        let icon = match status {
            "in_progress" => "●",
            _ => "○",
        };
        let truncated = truncate_to_width(content, 60);
        output.push_str(&format!(
            "       {} {}. {:<40} [{}]",
            icon,
            i + 1,
            truncated,
            status
        ));
        output.push('\n');
    }

    if completed > 0 {
        output.push_str(&format_collapse_line(&format!("{} completed hidden", completed)));
        output.push('\n');
    }

    output
}

// ── DIFF formatters ──────────────────────────────────────────────

fn format_edit_diff(args_json: &str) -> String {
    let args: serde_json::Value = serde_json::from_str(args_json).unwrap_or_default();
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
    let old = args.get("oldString").and_then(|v| v.as_str()).unwrap_or("");
    let new = args.get("newString").and_then(|v| v.as_str()).unwrap_or("");

    let mut output = format_panel_line("DIFF", path);
    output.push('\n');

    // Show context lines from old string
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // Show up to 3 context lines before change
    for line in old_lines.iter().take(1) {
        output.push_str(&format_context_line(line));
        output.push('\n');
    }

    // Show removed lines
    for line in old_lines.iter() {
        output.push_str(&format_removed_line(line));
        output.push('\n');
    }

    // Show added lines
    for line in new_lines.iter() {
        output.push_str(&format_added_line(line));
        output.push('\n');
    }

    output
}

fn format_multiedit_diff(args_json: &str) -> String {
    let args: serde_json::Value = serde_json::from_str(args_json).unwrap_or_default();
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");

    let edits = match args.get("edits").and_then(|v| v.as_array()) {
        Some(e) => e,
        None => return format_panel_line("DIFF", path),
    };

    let header = format!("{} ({} edits)", path, edits.len());
    let mut output = format_panel_line("DIFF", &header);
    output.push('\n');

    for edit in edits.iter().take(5) {
        let old = edit.get("oldString").and_then(|v| v.as_str()).unwrap_or("");
        let new = edit.get("newString").and_then(|v| v.as_str()).unwrap_or("");

        // Show removed → added
        let old_truncated = truncate_to_width(old, 60);
        let new_truncated = truncate_to_width(new, 60);
        output.push_str(&format!("       "));
        output.push_str(&format_removed_inline(&old_truncated));
        output.push_str(" → ");
        output.push_str(&format_added_inline(&new_truncated));
        output.push('\n');
    }

    if edits.len() > 5 {
        let remaining = edits.len() - 5;
        output.push_str(&format_collapse_line(&format!("{} more edits", remaining)));
        output.push('\n');
    }

    output
}

// ── Helper functions ─────────────────────────────────────────────

fn truncate_to_width(s: &str, max_width: usize) -> String {
    if s.chars().count() <= max_width {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_width).collect();
        format!("{}…", truncated)
    }
}

fn format_line_numbered(num: usize, width: usize, content: &str) -> String {
    if color_enabled() {
        format!("\x1b[36m{:>width$}\x1b[0m │ {}", num, content, width = width)
    } else {
        format!("{:>width$} │ {}", num, content, width = width)
    }
}

fn format_collapse_line(msg: &str) -> String {
    if color_enabled() {
        format!("       \x1b[90m⋮ {}\x1b[0m", msg)
    } else {
        format!("       ⋮ {}", msg)
    }
}

fn format_context_line(content: &str) -> String {
    format!("       {}", content)
}

fn format_removed_line(content: &str) -> String {
    if color_enabled() {
        format!("\x1b[31m-      {}\x1b[0m", content)
    } else {
        format!("-      {}", content)
    }
}

fn format_added_line(content: &str) -> String {
    if color_enabled() {
        format!("\x1b[32m+      {}\x1b[0m", content)
    } else {
        format!("+      {}", content)
    }
}

fn format_removed_inline(content: &str) -> String {
    if color_enabled() {
        format!("\x1b[31m{}\x1b[0m", content)
    } else {
        format!("-{}", content)
    }
}

fn format_added_inline(content: &str) -> String {
    if color_enabled() {
        format!("\x1b[32m{}\x1b[0m", content)
    } else {
        format!("+{}", content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_read_basic() {
        let args = r#"{"path":"src/main.rs","offset":80,"limit":30}"#;
        let result = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7";
        let output = format_preview("read", args, result, false).unwrap();
        assert!(output.contains("src/main.rs"));
        assert!(output.contains("PREV"));
        assert!(output.contains("2 more lines"));
    }

    #[test]
    fn preview_read_compact_hidden() {
        let args = r#"{"path":"src/main.rs"}"#;
        let result = "line 1\nline 2";
        assert!(format_preview("read", args, result, true).is_none());
    }

    #[test]
    fn preview_glob_basic() {
        let args = r#"{"pattern":"**/*.rs"}"#;
        let result = "src/main.rs\nsrc/lib.rs\nsrc/utils.rs";
        let output = format_preview("glob", args, result, false).unwrap();
        assert!(output.contains("3 files"));
        assert!(output.contains("src/main.rs"));
    }

    #[test]
    fn preview_glob_many_files_collapse() {
        let args = r#"{"pattern":"**/*.rs"}"#;
        let files: Vec<String> = (0..20).map(|i| format!("src/file_{}.rs", i)).collect();
        let result = files.join("\n");
        let output = format_preview("glob", args, &result, false).unwrap();
        assert!(output.contains("20 files"));
        assert!(output.contains("10 more files"));
    }

    #[test]
    fn preview_grep_basic() {
        let args = r#"{"pattern":"format_tool","include":"*.rs"}"#;
        let result = "src/a.rs:42:format_tool_call\nsrc/b.rs:10:format_tool_done";
        let output = format_preview("grep", args, result, false).unwrap();
        assert!(output.contains("2 matches"));
        assert!(output.contains("format_tool"));
    }

    #[test]
    fn preview_todo_read_with_count_prefix() {
        // Simulates the actual todo_read output format: "{count} todos\n{json_array}"
        let result = "1 todos\n[\n  {\n    \"id\": \"1\",\n    \"content\": \"Fix bug\",\n    \"status\": \"pending\",\n    \"priority\": \"high\"\n  }\n]";
        let output = format_preview("todo_read", "{}", result, false).unwrap();
        assert!(output.contains("1 pending"));
        assert!(output.contains("Fix bug"));
        assert!(output.contains("○"));
    }

    #[test]
    fn preview_todo_read_empty_list() {
        let result = "0 todos\n[]";
        let output = format_preview("todo_read", "{}", result, false).unwrap();
        assert!(output.contains("0 pending"));
    }

    #[test]
    fn preview_todo_write() {
        let args = r#"{"todos":[{"id":"1","content":"Fix bug","status":"completed","priority":"high"},{"id":"2","content":"Add tests","status":"pending","priority":"medium"}]}"#;
        let output = format_preview("todo_write", args, "", false).unwrap();
        assert!(output.contains("2 todos"));
        assert!(output.contains("Fix bug"));
        assert!(output.contains("✓"));
        assert!(output.contains("○"));
    }

    #[test]
    fn preview_unknown_tool_no_preview() {
        assert!(format_preview("bash", "{}", "output", false).is_none());
    }

    #[test]
    fn diff_edit_basic() {
        let args = r#"{"path":"panel_format.rs","oldString":"fn main()","newString":"fn hello()"}"#;
        let output = format_diff("edit", args, "", false).unwrap();
        assert!(output.contains("DIFF"));
        assert!(output.contains("panel_format.rs"));
        assert!(output.contains("fn main()"));
        assert!(output.contains("fn hello()"));
    }

    #[test]
    fn diff_multiedit_basic() {
        let args = r#"{"path":"panel_format.rs","edits":[{"oldString":"a","newString":"b"},{"oldString":"c","newString":"d"}]}"#;
        let output = format_diff("multiedit", args, "", false).unwrap();
        assert!(output.contains("2 edits"));
    }

    #[test]
    fn diff_compact_hidden() {
        let args = r#"{"path":"x.rs","oldString":"a","newString":"b"}"#;
        assert!(format_diff("edit", args, "", true).is_none());
    }

    #[test]
    fn diff_unknown_tool_no_diff() {
        assert!(format_diff("bash", "{}", "", false).is_none());
    }

    #[test]
    fn truncate_to_width_short() {
        assert_eq!(truncate_to_width("hello", 10), "hello");
    }

    #[test]
    fn truncate_to_width_long() {
        let result = truncate_to_width("abcdefghij", 5);
        assert_eq!(result, "abcde…");
    }
}
