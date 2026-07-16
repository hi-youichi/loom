use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;

use tool_core::tool_name::TOOL_WORKFLOW_EVENTS;
use tool_core::{
    Tool, ToolCallContent, ToolCallContext, ToolOutputHint, ToolOutputStrategy, ToolSourceError,
    ToolSpec,
};

use crate::common::instance_dir_arg;
use crate::runtime::WorkflowRuntime;

const DEFAULT_EVENTS_LIMIT: u64 = 50;
const MAX_EVENTS_LIMIT: u64 = 500;

pub struct WorkflowEventsTool {
    pub(crate) runtime: Arc<WorkflowRuntime>,
}

impl WorkflowEventsTool {
    pub fn new(config_template: agent::agent::AgentConfig) -> Self {
        Self {
            runtime: Arc::new(WorkflowRuntime::new(config_template)),
        }
    }
}

fn parse_events_offset(args: &Value) -> u64 {
    args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0)
}

fn parse_events_limit(args: &Value) -> u64 {
    let raw = args
        .get("events_limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_EVENTS_LIMIT);
    raw.clamp(1, MAX_EVENTS_LIMIT)
}

fn parse_events_types(args: &Value) -> Option<Vec<String>> {
    let v = args.get("types")?;
    if v.is_null() {
        return None;
    }
    let arr = v.as_array()?;
    let out: Vec<String> = arr
        .iter()
        .filter_map(|t| t.as_str().map(|s| s.to_string()))
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn parse_events_agent_id(args: &Value) -> Option<String> {
    let v = args.get("agent_id")?;
    if v.is_null() {
        return None;
    }
    v.as_str().map(|s| s.to_string())
}

fn event_matches_types(event: &Value, types: &HashSet<&str>) -> bool {
    event
        .get("type")
        .and_then(|t| t.as_str())
        .map(|t| types.contains(t))
        .unwrap_or(false)
}

fn event_matches_agent_id(event: &Value, agent_id: &str) -> bool {
    event
        .get("agent_id")
        .and_then(|a| a.as_str())
        .map(|s| s == agent_id)
        .unwrap_or(false)
}

#[async_trait]
impl Tool for WorkflowEventsTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_EVENTS
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_WORKFLOW_EVENTS.to_string(),
            description: Some(
                "Paginated, filtered access to the structured event stream of a \
                 workflow instance. Filters: `types` (array of event-type strings) \
                 and `agent_id`. Pagination: `offset` (skip N matching events) and \
                 `events_limit` (page size, 1..=500)."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "instance_dir": {
                        "type": "string",
                        "description": "Instance directory name returned by workflow_start or workflow_list."
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 0,
                        "default": 0,
                        "description": "Skip the first N matching events."
                    },
                    "events_limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 500,
                        "default": 50,
                        "description": "Page size (clamped to 500)."
                    },
                    "types": {
                        "type": ["array", "null"],
                        "items": {"type": "string"},
                        "description": "Restrict returned events to those whose `type` field is in this set."
                    },
                    "agent_id": {
                        "type": ["string", "null"],
                        "description": "Restrict returned events to those with this `agent_id`."
                    }
                },
                "required": ["instance_dir"]
            }),
            output_hint: Some(ToolOutputHint::preferred(ToolOutputStrategy::Inline)),
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let dir = instance_dir_arg(&args, "workflow_events")?;
        let path = self
            .runtime
            .resolve_instance_path(dir)
            .ok_or_else(|| ToolSourceError::InvalidInput(format!("Instance '{dir}' not found")))?;
        let events_path = path.join("events.jsonl");
        let offset = parse_events_offset(&args);
        let events_limit = parse_events_limit(&args);
        let types = parse_events_types(&args);
        let agent_id = parse_events_agent_id(&args);

        let mut filtered_count: u64 = 0;
        let mut returned: usize = 0;
        let mut events: Vec<Value> = Vec::new();

        if let Ok(file) = std::fs::File::open(&events_path) {
            let types_set: Option<HashSet<&str>> = types
                .as_ref()
                .map(|v| v.iter().map(|s| s.as_str()).collect());

            let reader = std::io::BufReader::new(file);
            use std::io::BufRead as _;
            for line in reader.lines() {
                let Ok(line) = line else { continue };
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let val: Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                if let Some(set) = &types_set {
                    if !event_matches_types(&val, set) {
                        continue;
                    }
                }
                if let Some(aid) = &agent_id {
                    if !event_matches_agent_id(&val, aid) {
                        continue;
                    }
                }

                filtered_count += 1;
                if filtered_count > offset && (returned as u64) < events_limit {
                    events.push(val);
                    returned += 1;
                }
            }
        }

        let next_offset = if offset + (returned as u64) < filtered_count {
            Some(offset + returned as u64)
        } else {
            None
        };

        Ok(ToolCallContent::Text(
            serde_json::to_string_pretty(&json!({
                "instance_dir": dir,
                "offset": offset,
                "events_limit": events_limit,
                "total_matching": filtered_count,
                "next_offset": next_offset,
                "events": events,
            }))
            .unwrap_or_default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn events_offset_default_zero() {
        assert_eq!(parse_events_offset(&json!({})), 0);
    }

    #[test]
    fn events_offset_explicit() {
        assert_eq!(parse_events_offset(&json!({"offset": 7})), 7);
    }

    #[test]
    fn events_limit_default_50() {
        assert_eq!(parse_events_limit(&json!({})), 50);
    }

    #[test]
    fn events_limit_clamps_above_max() {
        assert_eq!(parse_events_limit(&json!({"events_limit": 10_000})), 500);
    }

    #[test]
    fn events_limit_clamps_below_min() {
        assert_eq!(parse_events_limit(&json!({"events_limit": 0})), 1);
    }

    #[test]
    fn events_types_missing_is_none() {
        assert!(parse_events_types(&json!({})).is_none());
    }

    #[test]
    fn events_types_null_is_none() {
        assert!(parse_events_types(&json!({"types": null})).is_none());
    }

    #[test]
    fn events_types_empty_array_is_none() {
        assert!(parse_events_types(&json!({"types": []})).is_none());
    }

    #[test]
    fn events_types_collects_strings() {
        let t = parse_events_types(&json!({"types": ["a", "b"]})).unwrap();
        assert_eq!(t, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn events_agent_id_missing_is_none() {
        assert!(parse_events_agent_id(&json!({})).is_none());
    }

    #[test]
    fn events_agent_id_null_is_none() {
        assert!(parse_events_agent_id(&json!({"agent_id": null})).is_none());
    }

    #[test]
    fn events_agent_id_returns_some() {
        assert_eq!(
            parse_events_agent_id(&json!({"agent_id": "aid"})).unwrap(),
            "aid"
        );
    }

    fn ev(value: Value) -> Value {
        value
    }

    #[test]
    fn event_matches_types_filters_by_set() {
        let mut set = HashSet::new();
        set.insert("run_done");
        assert!(event_matches_types(&ev(json!({"type": "run_done"})), &set));
        assert!(!event_matches_types(&ev(json!({"type": "agent_started"})), &set));
    }

    #[test]
    fn event_matches_agent_id_compares_strictly() {
        assert!(event_matches_agent_id(&ev(json!({"agent_id": "a"})), "a"));
        assert!(!event_matches_agent_id(&ev(json!({"agent_id": "b"})), "a"));
        assert!(!event_matches_agent_id(&ev(json!({"type": "x"})), "a"));
    }
}