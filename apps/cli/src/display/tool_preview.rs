//! Tool PREVIEW and DIFF rendering.
//!
//! Generates inline previews and diffs between CALL and DONE lines,
//! following the UX spec in docs/tool-display-ux.md.
//!
//! PREVIEW: Shows a snapshot of tool output (read, glob, grep, etc.)
//! DIFF: Shows edit changes with red/green highlighting

use super::terminal::stderr_color_enabled;

fn color_enabled() -> bool {
    stderr_color_enabled()
}

/// Maximum number of preview lines before collapsing.
const MAX_PREVIEW_LINES: usize = 10;
/// Maximum number of glob files to show in preview.
const MAX_GLOB_FILES: usize = 10;
/// Maximum number of grep matches to show.
const MAX_GREP_MATCHES: usize = 5;

/// Generate a PREVIEW block for a tool, given its name, args JSON, and result text.
///
/// Returns None if the tool doesn't support preview or if in compact mode.
pub fn format_preview(
    tool_name: &str,
    args_json: &str,
    result: &str,
    compact: bool,
) -> Option<String> {
    if compact {
        return None;
    }

    match tool_name {
        "read" | "glob" | "grep" | "ls" => {
            if result.trim().is_empty() {
                return None;
            }
        }
        "todo_write" | "todo_read" => {} // These don't need result text
        "batch" => {
            if result.trim().is_empty() {
                return None;
            }
        }
        "write_file" => return None, // Diff is handled separately, skip preview
        _ => return None,
    }

    match tool_name {
        "read" => Some(format_read_preview(args_json, result)),
        "glob" => Some(format_glob_preview(args_json, result)),
        "grep" => Some(format_grep_preview(args_json, result)),
        "ls" => Some(format_ls_preview(result)),
        "todo_write" => Some(format_todo_write_preview(args_json)),
        "todo_read" => Some(format_todo_read_preview(result)),
        "batch" => Some(format_batch_preview(result)),
        _ => None,
    }
}

/// Generate a DIFF block for edit/multiedit/write_file tools.
///
/// Returns None if the tool doesn't support diff.
pub fn format_diff(
    tool_name: &str,
    args_json: &str,
    result: &str,
    compact: bool,
) -> Option<String> {
    if compact {
        return None;
    }

    match tool_name {
        "edit" => Some(format_edit_diff(args_json)),
        "multiedit" => Some(format_multiedit_diff(args_json)),
        "write_file" => format_write_file_diff(args_json, result),
        _ => None,
    }
}

// ── PREVIEW formatters ───────────────────────────────────────────

fn format_read_preview(args_json: &str, result: &str) -> String {
    let args: serde_json::Value = serde_json::from_str(args_json).unwrap_or_default();
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
    let offset = args.get("offset").and_then(|v| v.as_u64());
    let limit = args.get("limit").and_then(|v| v.as_u64());

    let lines: Vec<&str> = result.lines().collect();
    let total = lines.len();

    // Strip cat -n line number prefix ("  1\t...") from each line
    let stripped: Vec<&str> = lines.iter().map(|l| strip_cat_line_number(l)).collect();

    // Derive start_line: from offset arg, or from first line's embedded number
    let start_line = if let Some(o) = offset {
        o as usize
    } else {
        extract_cat_line_number(
            stripped.first().copied().unwrap_or("1"),
            lines.first().copied().unwrap_or("1"),
        )
        .unwrap_or(1)
    };

    // Header: shows path + line range context
    let header = match (offset, limit) {
        (Some(o), Some(l)) => format!("{} [{}:{} of {}]", path, o, o + l, total),
        (Some(o), None) => format!("{} [{}: of {}]", path, o, total),
        (None, Some(_)) => format!("{} ({} lines)", path, total),
        _ => format!("{} ({} lines)", path, total),
    };

    let mut output = String::new();
    output.push_str(&format_file_header(&header));
    output.push('\n');

    // Check if this is a markdown file — if so, render each line with markdown formatting
    let is_md = path.to_lowercase().ends_with(".md");
    let line_num_width = format!("{}", start_line + total.min(MAX_PREVIEW_LINES)).len();

    for (i, line) in stripped.iter().take(MAX_PREVIEW_LINES).enumerate() {
        let line_num = start_line + i;
        if is_md {
            // Render each line through markdown line-level rendering
            let rendered = render_markdown_line(line);
            let truncated = truncate_to_width(&rendered, 120);
            output.push_str(&format_line_numbered(line_num, line_num_width, &truncated));
        } else {
            let truncated = truncate_to_width(line, 100);
            output.push_str(&format_line_numbered(line_num, line_num_width, &truncated));
        }
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

    let _header = format!("{} ({} files)", pattern, total);
    let mut output = String::new();

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
    let files: std::collections::HashSet<&str> =
        matches.iter().filter_map(|l| l.split(':').next()).collect();
    let file_count = files.len();

    let _header = if file_count > 1 {
        format!("{} ({} matches in {} files)", pattern, total, file_count)
    } else {
        format!("{} ({} matches)", pattern, total)
    };

    let mut output = String::new();

    for m in matches.iter().take(MAX_GREP_MATCHES) {
        let truncated = truncate_to_width(m, 100);
        output.push_str(&format!("       {}", truncated));
        output.push('\n');
    }

    if total > MAX_GREP_MATCHES {
        let remaining = total - MAX_GREP_MATCHES;
        output.push_str(&format_collapse_line(&format!(
            "{} more matches",
            remaining
        )));
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
    let _header = format!("({} todos)", _total);
    let mut output = String::new();

    for (i, todo) in todos.iter().enumerate() {
        let status = todo
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("pending");
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
            return String::new();
        }
    };

    let _total = todos.len();
    let pending = todos
        .iter()
        .filter(|t| t.get("status").and_then(|v| v.as_str()) == Some("pending"))
        .count();
    let in_progress = todos
        .iter()
        .filter(|t| t.get("status").and_then(|v| v.as_str()) == Some("in_progress"))
        .count();
    let completed = todos
        .iter()
        .filter(|t| t.get("status").and_then(|v| v.as_str()) == Some("completed"))
        .count();

    let _header = format!(
        "({} pending, {} in_progress, {} completed)",
        pending, in_progress, completed
    );
    let mut output = String::new();

    for (i, todo) in todos.iter().enumerate() {
        let status = todo
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("pending");
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
        output.push_str(&format_collapse_line(&format!(
            "{} completed hidden",
            completed
        )));
        output.push('\n');
    }

    output
}

// ── Batch preview ────────────────────────────────────────────────

/// Parse batch result text and render sub-call previews.
///
/// The batch result uses `---BATCH[N] tool_name params_json---` as section
/// separators. Each section's body is the sub-call result text.
fn format_batch_preview(result: &str) -> String {
    let mut output = String::new();
    let mut current_tool = "";
    let mut current_args = "";
    let mut body_lines: Vec<&str> = Vec::new();

    let flush = |tool: &str, args: &str, body: &[&str], out: &mut String| {
        if tool.is_empty() {
            return;
        }
        let body_text = body.join("\n");
        let trimmed = body_text.trim();
        if trimmed.starts_with("error:") {
            out.push_str(&format!("       ✗ {}: {}\n", tool, trimmed));
        } else if let Some(preview) = format_preview(tool, args, trimmed, false) {
            out.push_str(&preview);
        } else {
            // Generic: first 3 lines
            let lines: Vec<&str> = trimmed.lines().take(3).collect();
            for line in lines {
                let truncated = truncate_to_width(line, 80);
                out.push_str(&format!("       {}\n", truncated));
            }
            let total = trimmed.lines().count();
            if total > 3 {
                out.push_str(&format_collapse_line(&format!("{} more lines", total - 3)));
                out.push('\n');
            }
        }
    };

    for line in result.lines() {
        if let Some(rest) = line.strip_prefix("---BATCH[") {
            // Flush previous section
            flush(current_tool, current_args, &body_lines, &mut output);

            // Parse: rest is "N] tool_name params_json---"
            body_lines.clear();
            if let Some(end_idx) = rest.find("---") {
                let header = &rest[..end_idx];
                // header: "N] tool_name params_json"
                let after_bracket = header.find(']').map(|i| i + 1).unwrap_or(0);
                let header_rest = header[after_bracket..].trim_start();
                // Split into tool_name and params_json
                if let Some(space) = header_rest.find(' ') {
                    current_tool = &header_rest[..space];
                    current_args = &header_rest[space + 1..];
                } else {
                    current_tool = header_rest;
                    current_args = "{}";
                }
            } else {
                current_tool = "";
                current_args = "{}";
            }
        } else if line == "---BATCH_END---" {
            flush(current_tool, current_args, &body_lines, &mut output);
            current_tool = "";
            current_args = "{}";
            body_lines.clear();
        } else {
            body_lines.push(line);
        }
    }

    // Flush any remaining section
    flush(current_tool, current_args, &body_lines, &mut output);

    output
}

// ── DIFF formatters ──────────────────────────────────────────────

/// Compute a lightweight git-style diff from old/new strings.
///
/// Produces context lines (unchanged) + `-` removed + `+` added lines,
/// similar to `git diff` but without line numbers.
fn compute_light_diff<'a>(old: &'a str, new: &'a str) -> Vec<DiffLine<'a>> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // Compute LCS-based diff to identify context lines
    let mut diff = Vec::new();
    let mut oi = 0usize;
    let mut ni = 0usize;

    while oi < old_lines.len() || ni < new_lines.len() {
        if oi < old_lines.len() && ni < new_lines.len() && old_lines[oi] == new_lines[ni] {
            // Context line (unchanged)
            diff.push(DiffLine::Context(old_lines[oi]));
            oi += 1;
            ni += 1;
        } else {
            // Emit remaining old lines as removed
            // Find the next matching point
            let next_match = find_next_match(&old_lines, &new_lines, oi, ni);
            match next_match {
                Some((om, nm)) => {
                    // Remove old[oi..om]
                    for line in old_lines.iter().take(om).skip(oi) {
                        diff.push(DiffLine::Removed(line));
                    }
                    // Add new[ni..nm]
                    for line in new_lines.iter().take(nm).skip(ni) {
                        diff.push(DiffLine::Added(line));
                    }
                    oi = om;
                    ni = nm;
                }
                None => {
                    // No more matches, emit remaining as removed/added
                    while oi < old_lines.len() {
                        diff.push(DiffLine::Removed(old_lines[oi]));
                        oi += 1;
                    }
                    while ni < new_lines.len() {
                        diff.push(DiffLine::Added(new_lines[ni]));
                        ni += 1;
                    }
                }
            }
        }
    }

    diff
}

/// Diff line kind.
enum DiffLine<'a> {
    Context(&'a str),
    Removed(&'a str),
    Added(&'a str),
}

/// Find the next pair of matching lines starting from (oi, ni).
fn find_next_match<'a>(
    old: &[&'a str],
    new: &[&'a str],
    oi: usize,
    ni: usize,
) -> Option<(usize, usize)> {
    // Look ahead up to 20 lines in both directions
    let lookahead = 20;
    for delta_o in 0..=lookahead.min(old.len() - oi) {
        for delta_n in 0..=lookahead.min(new.len() - ni) {
            if delta_o == 0 && delta_n == 0 {
                continue;
            }
            let co = oi + delta_o;
            let cn = ni + delta_n;
            if co < old.len() && cn < new.len() && old[co] == new[cn] {
                return Some((co, cn));
            }
        }
    }
    None
}

fn format_edit_diff(args_json: &str) -> String {
    let args: serde_json::Value = serde_json::from_str(args_json).unwrap_or_default();
    let _path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
    let old = args.get("oldString").and_then(|v| v.as_str()).unwrap_or("");
    let new = args.get("newString").and_then(|v| v.as_str()).unwrap_or("");

    let diff = compute_light_diff(old, new);
    let mut output = String::new();

    for line in &diff {
        match line {
            DiffLine::Context(s) => {
                output.push(' ');
                output.push_str(s);
            }
            DiffLine::Removed(s) => {
                output.push_str(&format_removed_line(s));
            }
            DiffLine::Added(s) => {
                output.push_str(&format_added_line(s));
            }
        }
        output.push('\n');
    }

    output
}

fn format_multiedit_diff(args_json: &str) -> String {
    let args: serde_json::Value = serde_json::from_str(args_json).unwrap_or_default();
    let _path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");

    let edits = match args.get("edits").and_then(|v| v.as_array()) {
        Some(e) => e,
        None => return String::new(),
    };

    let mut output = String::new();
    let mut first = true;

    for edit in edits {
        if first {
            first = false;
        } else {
            // Blank line separator between edits
            output.push('\n');
        }

        let old = edit.get("oldString").and_then(|v| v.as_str()).unwrap_or("");
        let new = edit.get("newString").and_then(|v| v.as_str()).unwrap_or("");

        let diff = compute_light_diff(old, new);
        for line in &diff {
            match line {
                DiffLine::Context(s) => {
                    output.push(' ');
                    output.push_str(s);
                }
                DiffLine::Removed(s) => {
                    output.push_str(&format_removed_line(s));
                }
                DiffLine::Added(s) => {
                    output.push_str(&format_added_line(s));
                }
            }
            output.push('\n');
        }
    }

    output
}

/// Format diff for write_file tool.
///
/// The result is a ToolCallContent::Diff serialized as JSON:
/// `{"type":"diff","path":"...","old_text":"...","new_text":"..."}`
///
/// For new files (old_text is null), shows all new content as + lines.
fn format_write_file_diff(args_json: &str, result: &str) -> Option<String> {
    // Try to parse result as ToolCallContent::Diff JSON
    #[derive(serde::Deserialize)]
    struct DiffContent {
        #[serde(rename = "type")]
        _type: Option<String>,
        path: Option<String>,
        #[serde(rename = "old_text")]
        old_text: Option<String>,
        #[serde(rename = "new_text")]
        new_text: Option<String>,
    }

    let diff_content: DiffContent = serde_json::from_str(result).ok()?;

    let old_text = diff_content.old_text.as_deref().unwrap_or("");
    let new_text = diff_content.new_text.as_deref().unwrap_or("");

    // Extract path from args_json as fallback (result may not have path)
    let args: serde_json::Value = serde_json::from_str(args_json).unwrap_or_default();
    let path = diff_content
        .path
        .or_else(|| args.get("path").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_else(|| "?".to_string());

    let diff = compute_light_diff(old_text, new_text);

    let mut output = String::new();

    // If old_text is empty, it's a new file — show friendly creation message
    if old_text.is_empty() {
        // Show file creation with content preview (no diff markers)
        output.push_str(&format_file_header(&format!("{} (new)", path)));
        output.push('\n');
        for line in new_text.lines() {
            let truncated = truncate_to_width(line, 100);
            output.push_str("  ");
            output.push_str(&truncated);
            output.push('\n');
        }
    } else {
        // Show diff for existing file modifications
        output.push_str(&format_file_header(&path));
        output.push('\n');
        for line in &diff {
            match line {
                DiffLine::Context(s) => {
                    let truncated = truncate_to_width(s, 100);
                    output.push(' ');
                    output.push_str(&truncated);
                }
                DiffLine::Removed(s) => {
                    let truncated = truncate_to_width(s, 100);
                    output.push_str(&format_removed_line(&truncated));
                }
                DiffLine::Added(s) => {
                    let truncated = truncate_to_width(s, 100);
                    output.push_str(&format_added_line(&truncated));
                }
            }
            output.push('\n');
        }
    }

    Some(output)
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

/// Strip `cat -n` line number prefix (e.g. "  42\tcontent" → "content").
fn strip_cat_line_number(line: &str) -> &str {
    // Pattern: optional leading spaces, digits, then a tab
    let trimmed = line.trim_start();
    if let Some(tab_pos) = trimmed.find('\t') {
        let maybe_num = &trimmed[..tab_pos];
        if maybe_num.chars().all(|c| c.is_ascii_digit()) && !maybe_num.is_empty() {
            return &trimmed[tab_pos + 1..];
        }
    }
    line
}

/// Extract line number from a `cat -n` formatted line (e.g. "  42\t...").
fn extract_cat_line_number(_stripped: &str, original: &str) -> Option<usize> {
    let trimmed = original.trim_start();
    if let Some(tab_pos) = trimmed.find('\t') {
        let maybe_num = &trimmed[..tab_pos];
        if maybe_num.chars().all(|c| c.is_ascii_digit()) && !maybe_num.is_empty() {
            return maybe_num.parse().ok();
        }
    }
    None
}

fn format_line_numbered(num: usize, width: usize, content: &str) -> String {
    if color_enabled() {
        format!(
            "\x1b[36m{:>width$}\x1b[0m │ {}",
            num,
            content,
            width = width
        )
    } else {
        format!("{:>width$} │ {}", num, content, width = width)
    }
}

fn format_file_header(header: &str) -> String {
    if color_enabled() {
        format!("\x1b[2m       ── {} ──\x1b[0m", header)
    } else {
        format!("       ── {} ──", header)
    }
}

fn format_collapse_line(msg: &str) -> String {
    if color_enabled() {
        format!("       \x1b[90m⋮ {}\x1b[0m", msg)
    } else {
        format!("       ⋮ {}", msg)
    }
}

fn format_removed_line(content: &str) -> String {
    if color_enabled() {
        format!("\x1b[31m- {}\x1b[0m", content)
    } else {
        format!("- {}", content)
    }
}

fn format_added_line(content: &str) -> String {
    if color_enabled() {
        format!("\x1b[32m+ {}\x1b[0m", content)
    } else {
        format!("+ {}", content)
    }
}

pub fn format_result_preview(
    _tool_name: &str,
    result: &str,
    _elapsed: Option<std::time::Duration>,
) -> String {
    let lines: Vec<&str> = result.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return String::new();
    }
    let mut output = String::new();
    for line in &lines {
        output.push_str(line);
        output.push('\n');
    }
    output
}

fn format_ls_preview(result: &str) -> String {
    let mut output = String::new();
    for line in result.lines() {
        let trimmed = line.trim_end();
        if trimmed.ends_with('/') {
            if color_enabled() {
                output.push_str(&format!("\x1b[36m{}\x1b[0m\n", line));
            } else {
                output.push_str(line);
                output.push('\n');
            }
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    output
}

/// Render a single line of markdown content for preview display.
///
/// Applies both line-level formatting (headings, lists, blockquotes) and
/// inline formatting (bold, italic, code, links) to a single line of text.
fn render_markdown_line(line: &str) -> String {
    use super::markdown::*;

    if let Some((level, content)) = parse_heading(line) {
        return format_heading(level, content);
    }
    if let Some(content) = parse_unordered_list_item(line) {
        return format_list_item("•", content);
    }
    if let Some((num, content)) = parse_ordered_list_item(line) {
        return format_list_item(&format!("{}.", num), content);
    }
    if let Some(content) = line.strip_prefix('>') {
        return format_blockquote(content.trim_start());
    }
    if is_horizontal_rule(line.trim()) {
        return format_horizontal_rule();
    }
    // Default: inline rendering (bold, italic, code, links)
    render_inline(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_read_basic() {
        let args = r#"{"path":"src/main.rs","offset":80,"limit":30}"#;
        let result = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7";
        let output = format_preview("read", args, result, false).unwrap();
        assert!(output.contains("line 1"));
        // Only 7 lines, MAX_PREVIEW_LINES=10, so no collapse line
        assert!(!output.contains("more lines"));
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
        assert!(output.contains("src/main.rs"));
        assert!(output.contains("src/lib.rs"));
    }

    #[test]
    fn preview_glob_many_files_collapse() {
        let args = r#"{"pattern":"**/*.rs"}"#;
        let files: Vec<String> = (0..20).map(|i| format!("src/file_{}.rs", i)).collect();
        let result = files.join("\n");
        let output = format_preview("glob", args, &result, false).unwrap();
        assert!(output.contains("src/file_0.rs"));
        assert!(output.contains("10 more files"));
    }

    #[test]
    fn preview_grep_basic() {
        let args = r#"{"pattern":"format_tool","include":"*.rs"}"#;
        let result = "src/a.rs:42:format_tool_call\nsrc/b.rs:10:format_tool_done";
        let output = format_preview("grep", args, result, false).unwrap();
        assert!(output.contains("format_tool_call"));
        assert!(output.contains("format_tool_done"));
    }

    #[test]
    fn preview_todo_read_with_count_prefix() {
        // Simulates the actual todo_read output format: "{count} todos\n{json_array}"
        let result = "1 todos\n[\n  {\n    \"id\": \"1\",\n    \"content\": \"Fix bug\",\n    \"status\": \"pending\",\n    \"priority\": \"high\"\n  }\n]";
        let output = format_preview("todo_read", "{}", result, false).unwrap();
        assert!(output.contains("Fix bug"));
        assert!(output.contains("○"));
    }

    #[test]
    fn preview_todo_read_empty_list() {
        let result = "0 todos\n[]";
        let output = format_preview("todo_read", "{}", result, false);
        assert!(output.is_some());
    }

    #[test]
    fn preview_todo_write() {
        let args = r#"{"todos":[{"id":"1","content":"Fix bug","status":"completed","priority":"high"},{"id":"2","content":"Add tests","status":"pending","priority":"medium"}]}"#;
        let output = format_preview("todo_write", args, "", false).unwrap();
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
        let args =
            r##"{"path":"panel_format.rs","oldString":"fn main()","newString":"fn hello()"}"##;
        let output = format_diff("edit", args, "", false).unwrap();
        // Note: format_removed_line/add_line uses "- " prefix (with space)
        assert!(output.contains("- fn main()"));
        assert!(output.contains("+ fn hello()"));
    }

    #[test]
    fn diff_multiedit_basic() {
        let args = r##"{"path":"panel_format.rs","edits":[{"oldString":"a","newString":"b"},{"oldString":"c","newString":"d"}]}"##;
        let output = format_diff("multiedit", args, "", false).unwrap();
        // Note: format_removed_line/add_line uses "- " prefix (with space)
        assert!(output.contains("- a"));
        assert!(output.contains("+ b"));
        assert!(output.contains("- c"));
        assert!(output.contains("+ d"));
        // Should have blank line separator between edits
        assert!(output.contains("\n\n"));
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

    // ── Light diff tests ──

    #[test]
    fn diff_edit_with_context_lines() {
        // Old has context + changed line; new has context + new line
        let args = r##"{"path":"test.rs","oldString":"fn foo() {\n    let x = 1;\n}\nfn bar() {","newString":"fn foo() {\n    let x = 2;\n}\nfn bar() {"}"##;
        let output = format_diff("edit", args, "", false).unwrap();
        // Context line "fn foo() {" should appear with space prefix
        assert!(output.contains(" fn foo()"));
        // Note: format_removed_line/add_line uses "- " prefix (with space)
        assert!(output.contains("-     let x = 1;"));
        assert!(output.contains("+     let x = 2;"));
        // Context line "fn bar()" should also appear
        assert!(output.contains(" fn bar()"));
    }

    #[test]
    fn diff_edit_replaced_multiline() {
        let args = r##"{"path":"test.rs","oldString":"line1\nline2\nline3","newString":"line1\nline2_new\nline3"}"##;
        let output = format_diff("edit", args, "", false).unwrap();
        assert!(output.contains(" line1"));
        // Note: format_removed_line/add_line uses "- " prefix (with space)
        assert!(output.contains("- line2"));
        assert!(output.contains("+ line2_new"));
        assert!(output.contains(" line3"));
    }

    #[test]
    fn strip_cat_line_number_basic() {
        assert_eq!(strip_cat_line_number("  1\tfn main() {"), "fn main() {");
        assert_eq!(strip_cat_line_number(" 42\thello"), "hello");
        assert_eq!(strip_cat_line_number("1\t# Title"), "# Title");
    }

    #[test]
    fn strip_cat_line_number_no_prefix() {
        assert_eq!(strip_cat_line_number("no line number"), "no line number");
        assert_eq!(strip_cat_line_number(""), "");
    }

    #[test]
    fn preview_read_strips_cat_line_numbers() {
        let args = r#"{"path":"README.md"}"#;
        let result = "  1\t# Loom\n  2\t\n  3\tA graph-based agent";
        let output = format_preview("read", args, result, false).unwrap();
        // Should show content without double line numbers
        assert!(output.contains("Loom"));
        assert!(output.contains("A graph-based agent"));
        // Should NOT contain the raw "1\t" prefix in the displayed content
        assert!(!output.contains("1\t# Loom"));
    }

    #[test]
    fn preview_read_md_renders_heading() {
        let args = r#"{"path":"README.md"}"#;
        let result = "# Project Title\n\nA **bold** description with `code`.";
        let output = format_preview("read", args, result, false).unwrap();
        // Should contain rendered heading (not raw #)
        assert!(output.contains("Project Title"));
        assert!(output.contains("bold"));
        assert!(output.contains("code"));
    }

    #[test]
    fn preview_read_md_renders_list_items() {
        let args = r#"{"path":"docs/guide.md"}"#;
        let result = "- Feature one\n- Feature two\n1. First\n2. Second";
        let output = format_preview("read", args, result, false).unwrap();
        assert!(output.contains("Feature one"));
        assert!(output.contains("Feature two"));
        assert!(output.contains("First"));
    }

    #[test]
    fn preview_read_md_renders_blockquote() {
        let args = r#"{"path":"notes.md"}"#;
        let result = "> This is a quote\nNormal text";
        let output = format_preview("read", args, result, false).unwrap();
        assert!(output.contains("This is a quote"));
        assert!(output.contains("Normal text"));
    }

    #[test]
    fn preview_read_non_md_no_rendering() {
        let args = r#"{"path":"src/main.rs"}"#;
        let result = "# This is not a heading but a comment\nfn main() {}";
        let output = format_preview("read", args, result, false).unwrap();
        // For .rs files, raw content should be shown as-is
        assert!(output.contains("# This is not a heading but a comment"));
    }

    #[test]
    fn preview_read_md_renders_inline_code() {
        let args = r#"{"path":"api.md"}"#;
        let result = "Use `cargo build` to compile.";
        let output = format_preview("read", args, result, false).unwrap();
        assert!(output.contains("cargo build"));
    }

    #[test]
    fn render_markdown_line_heading() {
        let result = render_markdown_line("## Section Title");
        assert!(result.contains("Section Title"));
    }

    #[test]
    fn render_markdown_line_list() {
        let result = render_markdown_line("- item text");
        assert!(result.contains("item text"));
    }

    #[test]
    fn render_markdown_line_plain() {
        let result = render_markdown_line("just plain text");
        assert_eq!(result, "just plain text");
    }

    // ── Batch preview tests ──

    #[test]
    fn preview_batch_with_read_subcall() {
        let batch_result = "---BATCH[1] read {\"path\":\"src/main.rs\"}---\nfn main() {\n    println!(\"hello\");\n}\n---BATCH_END---\n";
        let output = format_preview("batch", "{}", batch_result, false).unwrap();
        assert!(output.contains("main.rs"));
        assert!(output.contains("fn main()"));
    }

    #[test]
    fn preview_batch_with_error() {
        let batch_result = "---BATCH[1] fail_tool {}---\nerror: tool not found\n---BATCH_END---\n";
        let output = format_preview("batch", "{}", batch_result, false).unwrap();
        assert!(output.contains("✗"));
        assert!(output.contains("error:"));
    }

    #[test]
    fn preview_batch_generic_tool() {
        let batch_result =
            "---BATCH[1] bash {\"command\":\"echo hello\"}---\nhello\n---BATCH_END---\n";
        let output = format_preview("batch", "{}", batch_result, false).unwrap();
        // bash has no specific preview, so generic rendering
        assert!(output.contains("hello"));
    }

    #[test]
    fn preview_batch_multiple_subcalls() {
        let batch_result = "---BATCH[1] read {\"path\":\"a.rs\"}---\nfn a() {}\n---BATCH[2] grep {\"pattern\":\"TODO\"}---\na.rs:1:TODO fix\n---BATCH_END---\n";
        let output = format_preview("batch", "{}", batch_result, false).unwrap();
        assert!(output.contains("a.rs"));
        assert!(output.contains("TODO"));
    }

    // ── write_file diff tests ──

    #[test]
    fn diff_write_file_new_file() {
        // New file: old_text is empty, shows creation message with "(new)" suffix
        let args = r#"{"path":"new.rs","content":"fn main() {\n    println!(\"hello\");\n}"}"#;
        let result = r#"{"type":"diff","path":"new.rs","old_text":null,"new_text":"fn main() {\n    println!(\"hello\");\n}"}"#;
        let output = format_diff("write_file", args, result, false).unwrap();
        // Should show file path with "(new)" suffix in header
        assert!(output.contains("new.rs"));
        assert!(output.contains("(new)"));
        // Should show content WITHOUT + prefix (just "  " indentation)
        assert!(output.contains("  fn main()"));
        assert!(output.contains("  println!(\"hello\");"));
        assert!(output.contains("  }"));
        // Should NOT contain + prefix for new files
        assert!(!output.contains("+fn main()"));
    }

    #[test]
    fn diff_write_file_existing_file() {
        // Existing file: shows context, removed, and added lines
        let args = r##"{"path":"existing.rs"}"##;
        let result = r##"{"type":"diff","path":"existing.rs","old_text":"fn old() {\n    println!(\"old\");\n}","new_text":"fn new() {\n    println!(\"new\");\n}"}"##;
        let output = format_diff("write_file", args, result, false).unwrap();
        assert!(output.contains("existing.rs"));
        // Note: format_removed_line/add_line uses "- " prefix (with space)
        assert!(output.contains("- fn old()"));
        assert!(output.contains("+ fn new()"));
    }

    #[test]
    fn diff_write_file_invali_result_returns_none() {
        // Invalid result JSON should return None gracefully
        let args = r#"{"path":"test.rs"}"#;
        let output = format_diff("write_file", args, "not json", false);
        assert!(output.is_none());
    }
}
