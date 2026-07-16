use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

use tool_core::tool_name::TOOL_WORKFLOW_LIST;
use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

use crate::common::read_json_value;
use crate::runtime::WorkflowRuntime;

const DEFAULT_LIST_INSTANCES_LIMIT: usize = 20;
const MAX_LIST_INSTANCES_LIMIT: usize = 100;
const LIST_INSTANCES_STATUS_FILTERS: &[&str] = &["completed", "failed", "cancelled"];

pub struct WorkflowListTool {
    pub(crate) runtime: Arc<WorkflowRuntime>,
}

impl WorkflowListTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

fn parse_list_instances_limit(args: &Value) -> Result<usize, ToolSourceError> {
    let Some(v) = args.get("limit") else {
        return Ok(DEFAULT_LIST_INSTANCES_LIMIT);
    };
    if v.is_null() {
        return Ok(DEFAULT_LIST_INSTANCES_LIMIT);
    }
    let n = v.as_u64().ok_or_else(|| {
        ToolSourceError::InvalidInput(format!("'limit' must be a positive integer, got {v}"))
    })?;
    if !(1..=MAX_LIST_INSTANCES_LIMIT as u64).contains(&n) {
        return Err(ToolSourceError::InvalidInput(format!(
            "'limit' must be between 1 and {MAX_LIST_INSTANCES_LIMIT}, got {n}"
        )));
    }
    Ok(n as usize)
}

fn parse_list_instances_cursor(args: &Value) -> Option<String> {
    let v = args.get("cursor")?;
    if v.is_null() {
        return None;
    }
    v.as_str().filter(|s| !s.is_empty()).map(|s| s.to_string())
}

fn parse_list_instances_status_filter(args: &Value) -> Result<Option<String>, ToolSourceError> {
    let v = match args.get("status_filter") {
        None | Some(Value::Null) => return Ok(None),
        Some(v) => v,
    };
    let s = v.as_str().ok_or_else(|| {
        ToolSourceError::InvalidInput(format!("'status_filter' must be a string, got {v}"))
    })?;
    let lower = s.to_lowercase();
    if !LIST_INSTANCES_STATUS_FILTERS.contains(&lower.as_str()) {
        return Err(ToolSourceError::InvalidInput(format!(
            "'status_filter' must be one of completed|failed|cancelled, got {s}"
        )));
    }
    Ok(Some(lower))
}

fn build_entry_from_instance_json(v: &Value, dir_name: &str) -> Value {
    let wf = v.get("workflow");
    let kind = wf
        .and_then(|w| w.get("kind"))
        .and_then(|x| x.as_str())
        .unwrap_or("file")
        .to_string();
    let name = wf
        .and_then(|w| w.get("name"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    json!({
        "instance_id": v.get("instance_id").and_then(|x| x.as_str()).unwrap_or("?"),
        "instance_dir": dir_name,
        "status": v.get("status").and_then(|x| x.as_str()).unwrap_or("unknown"),
        "workflow": {
            "kind": kind,
            "name": name,
        },
        "created_at": v.get("created_at").and_then(|x| x.as_u64()).unwrap_or(0),
        "completed_at": v.get("completed_at").and_then(|x| x.as_u64()).unwrap_or(0),
        "total_tokens": v.get("total_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
        "agent_count": v.get("agent_count").and_then(|x| x.as_u64()).unwrap_or(0),
    })
}

fn build_entry_from_checkpoint(ckpt: &Value, dir_name: &str) -> Option<Value> {
    let status = ckpt
        .get("status")
        .and_then(|x| x.as_str())
        .unwrap_or("unknown");
    let is_terminal = matches!(
        status.to_ascii_lowercase().as_str(),
        "completed" | "failed" | "cancelled"
    );
    if !is_terminal {
        return None;
    }
    let agent_count = ckpt
        .get("agent_results")
        .and_then(|x| x.as_object())
        .map(|o| o.len() as u64)
        .unwrap_or(0);
    Some(json!({
        "instance_id": ckpt.get("run_id").and_then(|x| x.as_str()).unwrap_or("?"),
        "instance_dir": dir_name,
        "status": status,
        "workflow": {
            "kind": "file",
            "name": dir_name,
        },
        "created_at": ckpt.get("created_at").and_then(|x| x.as_u64()).unwrap_or(0),
        "completed_at": ckpt.get("updated_at").and_then(|x| x.as_u64()).unwrap_or(0),
        "total_tokens": ckpt.get("total_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
        "agent_count": agent_count,
    }))
}

fn collect_instances_under(root: &Path, out: &mut Vec<Value>) {
    if !root.exists() {
        return;
    }
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let inst_path = path.join("instance.json");
        if let Some(inst) = read_json_value(&inst_path) {
            out.push(build_entry_from_instance_json(&inst, &dir_name));
            continue;
        }

        let ckpt_path = path.join("checkpoint.json");
        if let Some(ckpt) = read_json_value(&ckpt_path) {
            if let Some(entry) = build_entry_from_checkpoint(&ckpt, &dir_name) {
                out.push(entry);
            }
        }
    }
}

#[async_trait]
impl Tool for WorkflowListTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_LIST
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_LIST.to_string(),
            description: Some(
                "List completed workflow instances with optional status filtering. \
                 Results are paginated by `limit` and opaque `cursor`, and include \
                 instance identifiers, status, workflow names, timestamps, token \
                 totals, and agent counts."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100,
                        "default": 20,
                        "description": "Max instances to return. Default: 20, max: 100."
                    },
                    "cursor": {
                        "type": "string",
                        "description": "Opaque cursor from a previous page's `next_cursor`."
                    },
                    "status_filter": {
                        "type": "string",
                        "enum": ["completed", "failed", "cancelled"],
                        "description": "Restrict to entries with this status. Case-insensitive."
                    }
                }
            }),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        list_instances(&self.runtime, &args)
    }
}

fn list_instances(
    runtime: &WorkflowRuntime,
    args: &Value,
) -> Result<ToolCallContent, ToolSourceError> {
    let limit = parse_list_instances_limit(args)?;
    let cursor = parse_list_instances_cursor(args);
    let status_filter = parse_list_instances_status_filter(args)?;

    let mut entries: Vec<Value> = Vec::new();
    collect_instances_under(&runtime.instances_root(), &mut entries);
    collect_instances_under(&runtime.runs_root(), &mut entries);

    if let Some(ref sf) = status_filter {
        let want = sf.to_lowercase();
        entries.retain(|e| {
            e.get("status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase() == want)
                .unwrap_or(false)
        });
    }

    entries.sort_by(|a, b| {
        let ca = a.get("created_at").and_then(|v| v.as_u64()).unwrap_or(0);
        let cb = b.get("created_at").and_then(|v| v.as_u64()).unwrap_or(0);
        cb.cmp(&ca).then_with(|| {
            let da = a.get("instance_dir").and_then(|v| v.as_str()).unwrap_or("");
            let db = b.get("instance_dir").and_then(|v| v.as_str()).unwrap_or("");
            db.cmp(da)
        })
    });

    let total_after_filter = entries.len();
    let start_idx = match cursor.as_ref() {
        None => 0,
        Some(c) => {
            let pos = entries.iter().position(|e| {
                e.get("instance_dir")
                    .and_then(|v| v.as_str())
                    .map(|s| s == c)
                    .unwrap_or(false)
            });
            match pos {
                None => {
                    return Err(ToolSourceError::ToolError(format!("cursor not found: {c}")));
                }
                Some(p) => p + 1,
            }
        }
    };

    let page: Vec<Value> = entries
        .iter()
        .skip(start_idx)
        .take(limit)
        .cloned()
        .collect();

    let next_cursor = if page.is_empty() {
        None
    } else if start_idx + page.len() < total_after_filter {
        page.last()
            .and_then(|v| v.get("instance_dir").and_then(|v| v.as_str()))
            .map(|s| s.to_string())
    } else {
        None
    };
    let has_more = next_cursor.is_some();

    Ok(ToolCallContent::Text(
        serde_json::to_string_pretty(&json!({
            "instances": page,
            "count": page.len(),
            "next_cursor": next_cursor,
            "has_more": has_more,
        }))
        .unwrap_or_default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn list_limit_default_when_missing() {
        assert_eq!(parse_list_instances_limit(&json!({})).unwrap(), 20);
    }

    #[test]
    fn list_limit_explicit_value() {
        assert_eq!(parse_list_instances_limit(&json!({"limit": 50})).unwrap(), 50);
    }

    #[test]
    fn list_limit_at_bounds() {
        assert_eq!(parse_list_instances_limit(&json!({"limit": 1})).unwrap(), 1);
        assert_eq!(parse_list_instances_limit(&json!({"limit": 100})).unwrap(), 100);
    }

    #[test]
    fn list_limit_rejects_zero() {
        assert!(parse_list_instances_limit(&json!({"limit": 0})).is_err());
    }

    #[test]
    fn list_limit_rejects_over_max() {
        assert!(parse_list_instances_limit(&json!({"limit": 101})).is_err());
    }

    #[test]
    fn list_limit_rejects_non_integer() {
        assert!(parse_list_instances_limit(&json!({"limit": "fast"})).is_err());
        assert!(parse_list_instances_limit(&json!({"limit": 4.5})).is_err());
        assert!(parse_list_instances_limit(&json!({"limit": -1})).is_err());
    }

    #[test]
    fn list_limit_null_treated_as_default() {
        assert_eq!(parse_list_instances_limit(&json!({"limit": null})).unwrap(), 20);
    }

    #[test]
    fn list_cursor_missing_returns_none() {
        assert!(parse_list_instances_cursor(&json!({})).is_none());
    }

    #[test]
    fn list_cursor_null_returns_none() {
        assert!(parse_list_instances_cursor(&json!({"cursor": null})).is_none());
    }

    #[test]
    fn list_cursor_empty_string_returns_none() {
        assert!(parse_list_instances_cursor(&json!({"cursor": ""})).is_none());
    }

    #[test]
    fn list_cursor_nonempty_returns_some() {
        assert_eq!(
            parse_list_instances_cursor(&json!({"cursor": "abc"})).unwrap(),
            "abc"
        );
    }

    #[test]
    fn list_status_filter_missing_returns_none() {
        assert!(parse_list_instances_status_filter(&json!({})).unwrap().is_none());
    }

    #[test]
    fn list_status_filter_null_returns_none() {
        assert!(parse_list_instances_status_filter(&json!({"status_filter": null})).unwrap().is_none());
    }

    #[test]
    fn list_status_filter_terminal_lowercased() {
        let f = parse_list_instances_status_filter(&json!({"status_filter": "FAILED"}))
            .unwrap()
            .unwrap();
        assert_eq!(f, "failed");
    }

    #[test]
    fn list_status_filter_rejects_unknown() {
        assert!(parse_list_instances_status_filter(&json!({"status_filter": "running"})).is_err());
    }

    #[test]
    fn list_status_filter_rejects_non_string() {
        assert!(parse_list_instances_status_filter(&json!({"status_filter": 5})).is_err());
    }

    fn sample_checkpoint(run_id: &str, status: &str, created_at: u64) -> Value {
        json!({
            "run_id": run_id,
            "status": status,
            "created_at": created_at,
            "updated_at": created_at + 1,
            "agent_results": {},
            "total_tokens": 100,
        })
    }

    #[test]
    fn list_entry_from_checkpoint_skips_non_terminal() {
        assert!(build_entry_from_checkpoint(
            &sample_checkpoint("r", "running", 1),
            "r"
        )
        .is_none());
    }

    #[test]
    fn list_entry_from_checkpoint_keeps_terminal() {
        let e = build_entry_from_checkpoint(
            &sample_checkpoint("r1", "completed", 2),
            "loom-instance_r1",
        )
        .unwrap();
        assert_eq!(e["instance_id"], "r1");
        assert_eq!(e["status"], "completed");
        assert_eq!(e["instance_dir"], "loom-instance_r1");
    }

    #[test]
    fn list_entry_from_instance_json_preserves_workflow() {
        let inst = json!({
            "instance_id": "r2",
            "status": "failed",
            "workflow": {"kind": "inline", "name": "demo"},
            "created_at": 5,
            "completed_at": 7,
            "total_tokens": 9,
            "agent_count": 2,
        });
        let e = build_entry_from_instance_json(&inst, "loom-instance_r2");
        assert_eq!(e["workflow"]["kind"], "inline");
        assert_eq!(e["workflow"]["name"], "demo");
        assert_eq!(e["agent_count"], 2);
    }

    fn write_checkpoint(dir: &Path, run_id: &str, status: &str, created_at: u64) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("checkpoint.json"),
            serde_json::to_vec_pretty(&sample_checkpoint(run_id, status, created_at)).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn collect_instances_under_skips_non_terminal_and_keeps_terminal() {
        let tmp = tempfile::tempdir().unwrap();
        let root: PathBuf = tmp.path().join(".luft").join("runs");
        write_checkpoint(&root.join("done"), "rd", "completed", 1);
        write_checkpoint(&root.join("alive"), "ra", "running", 2);
        let mut out = Vec::new();
        collect_instances_under(&root, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["instance_dir"], "done");
    }

    #[test]
    fn collect_instances_under_handles_missing_root() {
        let mut out = Vec::new();
        collect_instances_under(std::path::Path::new("/nonexistent/7c1a9f"), &mut out);
        assert!(out.is_empty());
    }
}