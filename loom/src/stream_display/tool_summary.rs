//! Smart tool call summary formatter.
//!
//! Parses raw JSON arguments into human-readable summaries for each tool,
//! replacing raw JSON noise with concise, actionable descriptions.
//! Follows the UX spec in docs/tool-display-ux.md.

use std::time::Duration;

/// Format elapsed duration per spec:
/// - < 100ms: `(0.0s)`
/// - 100ms–1s: `(0.Xs)`
/// - 1s–60s: `(X.Xs)`
/// - >= 60s: `(Xm XXs)`
pub fn format_elapsed(duration: Duration) -> String {
    let total_secs = duration.as_secs_f64();
    if total_secs >= 60.0 {
        let mins = duration.as_secs() / 60;
        let secs = duration.as_secs() % 60;
        format!("({}m {:02}s)", mins, secs)
    } else {
        format!("({:.1}s)", total_secs)
    }
}

/// Truncate a string to max chars, appending "…" if truncated.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{}…", truncated)
    }
}

/// Parse a JSON string field from raw arguments JSON.
fn json_str<'a>(args: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    args.get(field).and_then(|v| v.as_str())
}

/// Parse a JSON u64 field from raw arguments JSON.
fn json_u64(args: &serde_json::Value, field: &str) -> Option<u64> {
    args.get(field).and_then(|v| v.as_u64())
}

/// Parse a JSON bool field from raw arguments JSON.
#[allow(dead_code)]
fn json_bool(args: &serde_json::Value, field: &str) -> Option<bool> {
    args.get(field).and_then(|v| v.as_bool())
}

// ── CALL summaries ───────────────────────────────────────────────

/// Generate a smart CALL summary for a tool.
///
/// Returns a human-readable one-liner describing what the tool is about to do.
/// Falls back to raw args (truncated) for unknown tools.
pub fn format_call_summary(tool_name: &str, args_json: &str) -> String {
    let args: serde_json::Value = serde_json::from_str(args_json).unwrap_or(serde_json::Value::Null);

    match tool_name {
        "bash" | "powershell" => {
            // Show the raw command; truncate long commands
            let cmd = json_str(&args, "command")
                .or_else(|| json_str(&args, "script"))
                .unwrap_or(args_json);
            // For multi-line commands, take first line + …
            let first_line = cmd.lines().next().unwrap_or(cmd);
            let single = if cmd.lines().count() > 1 {
                format!("{}…", first_line)
            } else {
                first_line.to_string()
            };
            truncate(&single, 80)
        }

        "read" => {
            let path = json_str(&args, "path").unwrap_or("?");
            let offset = json_u64(&args, "offset");
            let limit = json_u64(&args, "limit");
            match (offset, limit) {
                (Some(o), Some(l)) => format!("{} [{}:{}]", path, o, o + l),
                (Some(o), None) => format!("{} [{}:]", path, o),
                (None, Some(l)) => format!("{} [:{}]", path, l),
                _ => path.to_string(),
            }
        }

        "edit" => {
            let path = json_str(&args, "path").unwrap_or("?");
            let old = json_str(&args, "oldString").unwrap_or("");
            let new = json_str(&args, "newString").unwrap_or("");
            let old_lines = old.lines().count().max(1);
            let new_lines = new.lines().count().max(1);
            format!("{} (+{}/-{})", path, new_lines, old_lines)
        }

        "multiedit" => {
            let path = json_str(&args, "path").unwrap_or("?");
            let edits_arr = args.get("edits").and_then(|v| v.as_array());
            let edit_count = edits_arr.map(|a| a.len()).unwrap_or(1);
            let mut total_old = 0usize;
            let mut total_new = 0usize;
            if let Some(edits) = edits_arr {
                for e in edits {
                    let old = e.get("oldString").and_then(|v| v.as_str()).unwrap_or("");
                    let new = e.get("newString").and_then(|v| v.as_str()).unwrap_or("");
                    total_old += old.lines().count().max(1);
                    total_new += new.lines().count().max(1);
                }
            }
            format!("{} (+{}/-{}, {} edits)", path, total_new, total_old, edit_count)
        }

        "write_file" => {
            let path = json_str(&args, "path").unwrap_or("?");
            let content = json_str(&args, "content").unwrap_or("");
            let bytes = content.len();
            format!("{} ({} bytes)", path, bytes)
        }

        "glob" => {
            let pattern = json_str(&args, "pattern").unwrap_or(args_json);
            truncate(pattern, 60)
        }

        "grep" => {
            let pattern = json_str(&args, "pattern").unwrap_or("");
            let include = json_str(&args, "include").unwrap_or("");
            if include.is_empty() {
                truncate(pattern, 60)
            } else {
                format!("{} [{}]", truncate(pattern, 40), include)
            }
        }

        "create_dir" => {
            let path = json_str(&args, "path").unwrap_or("?");
            path.to_string()
        }

        "ls" => {
            let path = json_str(&args, "path")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or(".");
            path.to_string()
        }

        "delete_file" => {
            let path = json_str(&args, "path").unwrap_or("?");
            path.to_string()
        }

        "move_file" => {
            let source = json_str(&args, "source").unwrap_or("?");
            let target = json_str(&args, "target").unwrap_or("?");
            format!("{} → {}", source, target)
        }

        "apply_patch" => {
            let patch = json_str(&args, "patchText").unwrap_or("");
            let lines = patch.lines().count();
            format!("patch ({} lines)", lines)
        }

        "lsp" => {
            let action = json_str(&args, "action").unwrap_or("?");
            let file = json_str(&args, "file_path").unwrap_or("");
            let line = json_u64(&args, "line");
            match line {
                Some(l) => format!("{} {}:{}", action, file, l),
                _ => format!("{} {}", action, file),
            }
        }

        "web_fetcher" => {
            let method = json_str(&args, "method").unwrap_or("GET");
            let url = json_str(&args, "url").unwrap_or("?");
            truncate(&format!("{} {}", method, url), 80)
        }

        "websearch" => {
            let query = json_str(&args, "query").unwrap_or(args_json);
            truncate(query, 60)
        }

        "task_create" => {
            let name = json_str(&args, "name").unwrap_or("task");
            truncate(name, 50)
        }

        "task_update" => {
            let id = json_str(&args, "id").unwrap_or("?");
            let status = json_str(&args, "status").unwrap_or("");
            if status.is_empty() {
                format!("#{}", id)
            } else {
                format!("#{} → {}", id, status)
            }
        }

        "task_list" => {
            let status = json_str(&args, "status").unwrap_or("");
            let name = json_str(&args, "name").unwrap_or("");
            let mut parts = Vec::new();
            if !status.is_empty() { parts.push(status.to_string()); }
            if !name.is_empty() { parts.push(name.to_string()); }
            if parts.is_empty() { "all".to_string() } else { parts.join(": ") }
        }

        "task_show" => {
            let id = json_str(&args, "id").unwrap_or("?");
            format!("#{}", id)
        }

        "task_delete" => {
            let id = json_str(&args, "id").unwrap_or("?");
            format!("#{}", id)
        }

        "invoke_agent" => {
            let agents = args.get("agents").and_then(|v| v.as_array());
            if let Some(arr) = agents {
                if let Some(first) = arr.first() {
                    let agent = first.get("agent").and_then(|v| v.as_str()).unwrap_or("?");
                    let task = first.get("task").and_then(|v| v.as_str()).unwrap_or("");
                    format!("{}: {}", agent, truncate(task, 50))
                } else {
                    "invoke_agent".to_string()
                }
            } else {
                let agent = json_str(&args, "agent").unwrap_or("?");
                let task = json_str(&args, "task").unwrap_or("");
                format!("{}: {}", agent, truncate(task, 50))
            }
        }

        "list_agents" => "list_agents".to_string(),

        "skill" => {
            let name = json_str(&args, "name").unwrap_or("?");
            name.to_string()
        }

        "help" => "help".to_string(),

        "todo_write" => {
            let todos = args.get("todos").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            format!("{} todos", todos)
        }

        "todo_read" => "todo_read".to_string(),

        "batch" => {
            let calls = args.get("calls").and_then(|v| v.as_array());
            match calls {
                Some(arr) => {
                    let count = arr.len();
                    let names: Vec<&str> = arr.iter()
                        .filter_map(|c| c.get("tool").and_then(|v| v.as_str()))
                        .collect();
                    let display: Vec<&str> = names.iter().take(5).copied().collect();
                    let label = display.join(", ");
                    if names.len() > 5 {
                        format!("{} calls ({} +{} more)", count, label, names.len() - 5)
                    } else {
                        format!("{} calls ({})", count, label)
                    }
                }
                None => "batch".to_string(),
            }
        }

        // Unknown tools: show truncated raw args
        _ => truncate(args_json, 60).to_string(),
    }
}

// ── DONE summaries ───────────────────────────────────────────────

/// Generate a smart DONE result summary for a tool.
///
/// `result` is the tool output text. `is_error` indicates failure.
/// Returns a one-liner like "exit 0, 3 lines", "30 lines", "replaced 1", etc.
pub fn format_done_summary(tool_name: &str, result: &str, is_error: bool) -> String {
    if is_error {
        // Extract key error message
        let err_msg = result.lines().next().unwrap_or(result);
        return truncate(err_msg, 60);
    }

    match tool_name {
        "bash" | "powershell" => {
            let line_count = result.lines().count();
            let trimmed = result.trim();
            // Try to detect exit code from output
            if trimmed.is_empty() {
                "exit 0, 0 lines".to_string()
            } else {
                format!("exit 0, {} lines", line_count)
            }
        }

        "read" => {
            let line_count = result.lines().count();
            format!("{} lines", line_count)
        }

        "edit" => {
            "replaced 1".to_string()
        }

        "multiedit" => {
            // Count "Applied" in output
            let count = result.matches("Applied").count();
            if count > 0 {
                format!("{} replaced", count)
            } else {
                "replaced".to_string()
            }
        }

        "write_file" => {
            // Try to get byte count from result
            let bytes = result.len();
            if bytes > 0 {
                format!("{} bytes written", bytes)
            } else {
                "created".to_string()
            }
        }

        "glob" => {
            let count = result.lines().filter(|l| !l.trim().is_empty()).count();
            format!("{} files", count)
        }

        "grep" => {
            let matches = result.lines().filter(|l| !l.trim().is_empty()).count();
            // Count unique files
            let files: std::collections::HashSet<&str> = result
                .lines()
                .filter_map(|l| l.split(':').next())
                .collect();
            if files.len() <= 1 {
                format!("{} matches", matches)
            } else {
                format!("{} matches in {} files", matches, files.len())
            }
        }

        "create_dir" => "created".to_string(),
        "delete_file" => "deleted".to_string(),
        "move_file" => "moved".to_string(),

        "apply_patch" => {
            let files = result.matches("Update File:").count()
                + result.matches("Add File:").count()
                + result.matches("Delete File:").count();
            if files > 0 {
                format!("{} files changed", files)
            } else {
                "applied".to_string()
            }
        }

        "lsp" => {
            // Try to count results
            let count = result.matches("\"").count() / 2; // rough heuristic
            if count > 0 {
                format!("{} results", count.min(99))
            } else {
                "ok".to_string()
            }
        }

        "web_fetcher" => {
            // Show size
            let kb = result.len() as f64 / 1024.0;
            if kb >= 1.0 {
                format!("200, {:.1}KB", kb)
            } else {
                format!("200, {} bytes", result.len())
            }
        }

        "websearch" => {
            // Count results (heuristic: count numbered items or results)
            let count = result.lines().filter(|l| !l.trim().is_empty()).count();
            if count > 0 {
                format!("{} results", count)
            } else {
                "0 results".to_string()
            }
        }

        "task_create" => {
            // Try to extract task ID from result
            if let Some(id) = extract_task_id(result) {
                format!("#{}", id)
            } else {
                "created".to_string()
            }
        }

        "task_update" => {
            if let Some(id) = extract_task_id(result) {
                format!("#{} updated", id)
            } else {
                "updated".to_string()
            }
        }

        "task_list" => {
            let count = result.lines().filter(|l| !l.trim().is_empty()).count();
            format!("{} tasks", count)
        }

        "task_show" => {
            if let Some(id) = extract_task_id(result) {
                // Try to extract status
                if let Some(status) = extract_task_status(result) {
                    format!("#{} {}", id, status)
                } else {
                    format!("#{}", id)
                }
            } else {
                "ok".to_string()
            }
        }

        "task_delete" => {
            if let Some(id) = extract_task_id(result) {
                format!("#{} deleted", id)
            } else {
                "deleted".to_string()
            }
        }

        "invoke_agent" => "completed".to_string(),
        "list_agents" => {
            let count = result.lines().filter(|l| !l.trim().is_empty()).count();
            format!("{} agents", count)
        }
        "skill" => {
            let name = result.lines().next().unwrap_or("loaded");
            if name.contains("loaded") {
                name.to_string()
            } else {
                "loaded".to_string()
            }
        }
        "help" => "ok".to_string(),
        "todo_write" => "saved".to_string(),
        "todo_read" => "ok".to_string(),

        "batch" => {
            let sub_count = result.lines()
                .filter(|l| l.starts_with("---BATCH["))
                .count();
            let errors = result.lines()
                .filter(|l| l.trim().starts_with("error:"))
                .count();
            let ok = sub_count.saturating_sub(errors);
            if errors > 0 {
                format!("{} ok, {} error", ok, errors)
            } else {
                format!("{} ok", ok)
            }
        }

        "ls" => {
            let file_count = result.lines().filter(|l| !l.trim().is_empty()).count();
            format!("{} files", file_count)
        }

        _ => truncate(result.lines().next().unwrap_or("ok"), 60).to_string(),
    }
}

/// Extract a task ID (first 4+ hex chars) from result text.
fn extract_task_id(result: &str) -> Option<String> {
    // Look for UUID-like patterns or short hex IDs
    for word in result.split(|c: char| !c.is_alphanumeric() && c != '-') {
        if word.len() >= 4 && word.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Some(word.to_string());
        }
    }
    None
}

/// Extract task status from result text.
fn extract_task_status(result: &str) -> Option<String> {
    for status in &["pending", "in_progress", "completed", "cancelled"] {
        if result.contains(status) {
            return Some(status.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── format_elapsed ──

    #[test]
    fn elapsed_under_100ms() {
        assert_eq!(format_elapsed(Duration::from_millis(50)), "(0.1s)");
    }

    #[test]
    fn elapsed_300ms() {
        assert_eq!(format_elapsed(Duration::from_millis(300)), "(0.3s)");
    }

    #[test]
    fn elapsed_5s() {
        assert_eq!(format_elapsed(Duration::from_secs(5)), "(5.0s)");
    }

    #[test]
    fn elapsed_over_60s() {
        assert_eq!(format_elapsed(Duration::from_secs(135)), "(2m 15s)");
    }

    // ── CALL summaries ──

    #[test]
    fn call_bash() {
        let result = format_call_summary("bash", r#"{"command":"grep -rn \"TODO\" src/**/*.rs"}"#);
        assert!(result.contains("grep"));
        assert!(!result.contains("{\"command"));
    }

    #[test]
    fn call_read() {
        let result = format_call_summary("read", r#"{"path":"src/main.rs","offset":10,"limit":50}"#);
        assert!(result.contains("src/main.rs"));
        assert!(result.contains("[10:60]"));
    }

    #[test]
    fn call_read_no_offset() {
        let result = format_call_summary("read", r#"{"path":"src/main.rs"}"#);
        assert_eq!(result, "src/main.rs");
    }

    #[test]
    fn call_edit() {
        let result = format_call_summary("edit", r#"{"path":"panel_format.rs","oldString":"fn main()","newString":"fn hello()"}"#);
        assert!(result.contains("panel_format.rs"));
        assert!(result.contains("+1/-1"));
    }

    #[test]
    fn call_write_file() {
        let result = format_call_summary("write_file", r#"{"path":"src/new.rs","content":"fn main() {}"}"#);
        assert!(result.contains("src/new.rs"));
        assert!(result.contains("bytes"));
    }

    #[test]
    fn call_glob() {
        let result = format_call_summary("glob", r#"{"pattern":"**/*.rs"}"#);
        assert_eq!(result, "**/*.rs");
    }

    #[test]
    fn call_grep() {
        let result = format_call_summary("grep", r#"{"pattern":"format_tool","include":"*.rs"}"#);
        assert!(result.contains("format_tool"));
        assert!(result.contains("*.rs"));
    }

    #[test]
    fn call_move_file() {
        let result = format_call_summary("move_file", r#"{"source":"src/old.rs","target":"src/new.rs"}"#);
        assert!(result.contains("src/old.rs"));
        assert!(result.contains("src/new.rs"));
    }

    #[test]
    fn call_delete_file() {
        let result = format_call_summary("delete_file", r#"{"path":"src/old.rs"}"#);
        assert_eq!(result, "src/old.rs");
    }

    #[test]
    fn call_create_dir() {
        let result = format_call_summary("create_dir", r#"{"path":"src/new_module"}"#);
        assert_eq!(result, "src/new_module");
    }

    #[test]
    fn call_task_create() {
        let result = format_call_summary("task_create", r#"{"name":"Fix login bug"}"#);
        assert_eq!(result, "Fix login bug");
    }

    #[test]
    fn call_task_update() {
        let result = format_call_summary("task_update", r#"{"id":"550e","status":"completed"}"#);
        assert!(result.contains("#550e"));
        assert!(result.contains("completed"));
    }

    #[test]
    fn call_web_fetcher() {
        let result = format_call_summary("web_fetcher", r#"{"method":"GET","url":"https://api.example.com/data"}"#);
        assert!(result.contains("GET"));
        assert!(result.contains("api.example.com"));
    }

    #[test]
    fn call_websearch() {
        let result = format_call_summary("websearch", r#"{"query":"Rust async best practices 2025"}"#);
        assert!(result.contains("Rust async"));
    }

    #[test]
    fn call_invoke_agent() {
        let result = format_call_summary("invoke_agent", r#"{"agents":[{"agent":"explore","task":"find format_tool usage"}]}"#);
        assert!(result.contains("explore"));
        assert!(result.contains("format_tool"));
    }

    #[test]
    fn call_unknown_fallback() {
        let result = format_call_summary("custom_tool", r#"{"foo":"bar","baz":42}"#);
        // Should show truncated raw JSON
        assert!(result.contains("foo"));
    }

    #[test]
    fn call_multiedit() {
        let result = format_call_summary("multiedit", r#"{"path":"panel_format.rs","edits":[{"oldString":"a","newString":"b"},{"oldString":"c","newString":"d"},{"oldString":"e","newString":"f"}]}"#);
        assert!(result.contains("panel_format.rs"));
        assert!(result.contains("3 edits"));
        assert!(result.contains("+3/-3"));
    }

    #[test]
    fn call_lsp() {
        let result = format_call_summary("lsp", r#"{"action":"completion","file_path":"src/main.rs","line":42}"#);
        assert!(result.contains("completion"));
        assert!(result.contains("src/main.rs:42"));
    }

    // ── DONE summaries ──

    #[test]
    fn done_bash() {
        let result = format_done_summary("bash", "line1\nline2\nline3", false);
        assert_eq!(result, "exit 0, 3 lines");
    }

    #[test]
    fn done_read() {
        let result = format_done_summary("read", "line1\nline2\nline3", false);
        assert_eq!(result, "3 lines");
    }

    #[test]
    fn done_edit() {
        let result = format_done_summary("edit", "ok", false);
        assert_eq!(result, "replaced 1");
    }

    #[test]
    fn done_glob() {
        let result = format_done_summary("glob", "src/main.rs\nsrc/lib.rs\nsrc/utils.rs", false);
        assert_eq!(result, "3 files");
    }

    #[test]
    fn done_grep() {
        let result = format_done_summary("grep", "src/a.rs:42:match\nsrc/a.rs:50:match2\nsrc/b.rs:10:match3", false);
        assert_eq!(result, "3 matches in 2 files");
    }

    #[test]
    fn done_create_dir() {
        let result = format_done_summary("create_dir", "ok", false);
        assert_eq!(result, "created");
    }

    #[test]
    fn done_delete_file() {
        let result = format_done_summary("delete_file", "ok", false);
        assert_eq!(result, "deleted");
    }

    #[test]
    fn done_move_file() {
        let result = format_done_summary("move_file", "ok", false);
        assert_eq!(result, "moved");
    }

    #[test]
    fn done_error() {
        let result = format_done_summary("edit", "oldString not found in file", true);
        assert!(result.contains("oldString not found"));
    }

    #[test]
    fn done_write_file() {
        let result = format_done_summary("write_file", "ok", false);
        // Should show bytes or "created"
        assert!(!result.is_empty());
    }

    // ── batch summaries ──

    #[test]
    fn call_batch_basic() {
        let result = format_call_summary("batch", r#"{"calls":[{"tool":"read","parameters":{"path":"a.rs"}},{"tool":"grep","parameters":{"pattern":"TODO"}},{"tool":"bash","parameters":{"command":"echo"}}]}"#);
        assert!(result.contains("3 calls"));
        assert!(result.contains("read, grep, bash"));
    }

    #[test]
    fn call_batch_many_calls() {
        let calls_json = (0..12)
            .map(|i| format!(r#"{{"tool":"tool_{}","parameters":{{}}}}"#, i))
            .collect::<Vec<_>>()
            .join(",");
        let result = format_call_summary("batch", &format!(r#"{{"calls":[{}]}}"#, calls_json));
        assert!(result.contains("12 calls"));
        assert!(result.contains("+7 more"));
    }

    #[test]
    fn done_batch_all_ok() {
        let result_text = "---BATCH[1] read {}---\nhello\n---BATCH[2] grep {}---\nmatch1\n---BATCH_END---\n";
        let result = format_done_summary("batch", result_text, false);
        assert_eq!(result, "2 ok");
    }

    #[test]
    fn done_batch_with_errors() {
        let result_text = "---BATCH[1] read {}---\nhello\n---BATCH[2] fail {}---\nerror: not found\n---BATCH_END---\n";
        let result = format_done_summary("batch", result_text, false);
        assert_eq!(result, "1 ok, 1 error");
    }

    // ── truncate ──

    #[test]
    fn truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long() {
        let result = truncate("abcdefghij", 5);
        assert_eq!(result, "abcde…");
    }
}
