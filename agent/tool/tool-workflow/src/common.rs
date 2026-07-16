//! Cross-tool helpers shared by the six specialized workflow tools:
//! argument parsing, instance-dir validation, terminal-status helpers,
//! public-output sanitization, and the "running" receipt.

use serde_json::{json, Value};
use std::path::Path;
use tool_core::{ToolCallContent, ToolSourceError};

use crate::runtime::TERMINAL_STATUSES;

pub(crate) fn validate_instance_dir_name(name: &str) -> Result<&str, ToolSourceError> {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(ToolSourceError::InvalidInput(format!(
            "'instance_dir' must be a single path segment, got '{name}'"
        )));
    }
    Ok(name)
}

pub(crate) fn instance_dir_arg<'a>(
    args: &'a Value,
    action: &str,
) -> Result<&'a str, ToolSourceError> {
    let dir = args
        .get("instance_dir")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            ToolSourceError::InvalidInput(format!("'instance_dir' is required for {action}."))
        })?;
    validate_instance_dir_name(dir)?;
    Ok(dir)
}

pub(crate) fn is_terminal_status(status: &str) -> bool {
    TERMINAL_STATUSES.contains(&status.to_ascii_lowercase().as_str())
}

pub(crate) fn is_terminal_checkpoint(path: &Path) -> Option<bool> {
    let bytes = std::fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let status = value.get("status").and_then(Value::as_str)?;
    Some(is_terminal_status(status))
}

pub(crate) fn truncate_for_preview(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut cut = max_bytes;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + 16);
    out.push_str(&s[..cut]);
    out.push('…');
    out
}

pub(crate) fn running_receipt(instance_dir: &str) -> ToolCallContent {
    let payload = json!({
        "instance_dir": instance_dir,
        "status": "running",
    });
    ToolCallContent::Text(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

pub fn sanitize_instance_for_public(mut value: Value) -> Value {
    if let Some(wf) = value.get_mut("workflow").and_then(|v| v.as_object_mut()) {
        wf.remove("path");
    }
    if let Some(agents) = value.get_mut("agents").and_then(|v| v.as_array_mut()) {
        for a in agents {
            if let Some(obj) = a.as_object_mut() {
                obj.remove("output_ref");
            }
        }
    }
    if let Some(report) = value.get_mut("report").and_then(|v| v.as_object_mut()) {
        if report.contains_key("ref") && report.contains_key("preview") {
            report.remove("ref");
        }
    }
    if let Some(obj) = value.as_object_mut() {
        obj.remove("checkpoint_hash");
    }
    value
}

pub(crate) fn read_json_value(path: &Path) -> Option<Value> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn truncate_short_input_returned_unchanged() {
        let s = "hello world";
        assert_eq!(truncate_for_preview(s, 800), s);
    }

    #[test]
    fn truncate_long_input_with_ellipsis() {
        let s: String = "a".repeat(1500);
        let out = truncate_for_preview(&s, 800);
        assert!(out.ends_with('…'));
        assert!(out.starts_with(&"a".repeat(800)));
    }

    #[test]
    fn truncate_preserves_multibyte_boundaries() {
        let emoji = "🦀".repeat(2000);
        let out = truncate_for_preview(&emoji, 100);
        let prefix = out.trim_end_matches('…');
        for ch in prefix.chars() {
            assert_eq!(ch, '🦀');
        }
    }

    #[test]
    fn truncate_zero_width_is_just_ellipsis() {
        assert_eq!(truncate_for_preview("xxxx", 0), "…");
    }

    #[test]
    fn validate_instance_dir_rejects_empty_and_path_separators() {
        assert!(validate_instance_dir_name("").is_err());
        assert!(validate_instance_dir_name("a/b").is_err());
        assert!(validate_instance_dir_name("a\\b").is_err());
        assert!(validate_instance_dir_name("ok_dir").is_ok());
    }

    #[test]
    fn sanitize_strips_internal_file_refs() {
        let raw = json!({
            "schema_version": 1,
            "instance_id": "run-1",
            "instance_dir": "loom-instance_1",
            "workflow": {"kind": "file", "name": "wf", "path": "/abs/path/wf.lua"},
            "agents": [
                {"agent_id": "a", "output_ref": "agent-outputs/a.txt", "output_size": 4096}
            ],
            "report": {"ref": "report.json", "preview": "hello", "value_type": "object", "size_bytes": 4096},
            "checkpoint_hash": "deadbeef",
        });
        let cleaned = sanitize_instance_for_public(raw);
        assert!(cleaned["workflow"].get("path").is_none());
        assert!(cleaned["agents"][0].get("output_ref").is_none());
        assert!(cleaned["report"].get("ref").is_none());
        assert_eq!(cleaned["report"]["preview"].as_str().unwrap(), "hello");
        assert!(cleaned.get("checkpoint_hash").is_none());
    }

    #[test]
    fn sanitize_keeps_inline_report_content() {
        let raw = json!({
            "schema_version": 1,
            "report": {"ok": true, "verdict": "approved"},
            "workflow": {"kind": "inline", "name": "script"},
        });
        let cleaned = sanitize_instance_for_public(raw);
        assert_eq!(cleaned["report"]["ok"], true);
        assert_eq!(cleaned["report"]["verdict"], "approved");
    }
}