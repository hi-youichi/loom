//! Accumulates tool calls from streamed deltas into complete [`ToolCall`] values.
//!
//! Used by both `ChatOpenAI` and `ChatOpenAICompat` when processing SSE
//! tool_calls delta chunks.

use std::collections::HashMap;

use crate::state::ToolCall;

use tracing::{debug, warn};

/// Accumulates tool call deltas by index during streaming.
///
/// Each streamed chunk may contain partial tool call data (id, function name
/// fragment, argument fragment). This struct merges them by index and produces
/// the final list when streaming completes.
pub(crate) struct ToolCallAccumulator {
    /// index → (id, name, arguments)
    map: HashMap<u32, (String, String, String)>,
}

/// One delta from the LLM stream, provider-agnostic.
pub(crate) struct RawToolCallDelta {
    pub index: u32,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
}

impl ToolCallAccumulator {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Merge one delta into the accumulator.
    pub fn push(&mut self, delta: RawToolCallDelta) {
        let entry = self.map.entry(delta.index).or_insert_with(|| {
            (
                delta.id.clone().unwrap_or_default(),
                String::new(),
                String::new(),
            )
        });
        if let Some(ref id) = delta.id {
            if !id.is_empty() {
                entry.0 = id.clone();
            }
        }
        if let Some(name) = delta.name {
            entry.1.push_str(&name);
        }
        if let Some(args) = delta.arguments {
            entry.2.push_str(&args);
        }
    }

    /// Returns true if no tool calls have been accumulated.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Consume the accumulator and produce sorted `Vec<ToolCall>`.
    ///
    /// Tool calls are sorted by name for deterministic order.
    pub fn finish(self) -> Vec<ToolCall> {
        let mut tool_calls: Vec<ToolCall> = self
            .map
            .into_values()
            .map(|(id, name, arguments)| {
                let id_opt = if id.is_empty() { None } else { Some(id) };
                let arguments = sanitize_arguments(id_opt.as_deref(), &name, &arguments);
                ToolCall {
                    name,
                    arguments,
                    id: id_opt,
                }
            })
            .collect();
        tool_calls.sort_by(|a, b| a.name.cmp(&b.name));
        tool_calls
    }

    /// Replace all accumulated tool calls with an externally-provided list.
    ///
    /// The raw arguments are stored **without** sanitization; sanitization is
    /// deferred to `finish()` to ensure single-pass normalization.
    pub fn replace_from_vec(&mut self, tool_calls: Vec<ToolCall>) {
        self.map.clear();
        for (i, tc) in tool_calls.into_iter().enumerate() {
            self.map.insert(
                i as u32,
                (tc.id.unwrap_or_default(), tc.name, tc.arguments),
            );
        }
    }
}

impl Default for ToolCallAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalize tool_call.arguments to ensure it's a valid JSON string.
///
/// **Triggers**:
/// - MiniMax-M3 and similar models occasionally emit non-JSON content in
///   `function.arguments`, causing API error `2013 invalid function arguments json string`
///
/// **Behavior**:
/// 1. Empty / whitespace → `"{}"`
/// 2. Valid JSON string → return as-is (zero-overhead fast path)
/// 3. Parse failure → wrap to `{"_raw_args": "<original>"}` with warn log
///
/// **Round-trip safety**:
/// - If arguments already contain a `_raw_args` key (from a previous round's wrapping),
///   the value is **unwrapped** rather than double-wrapped, to prevent nesting.
///   This ensures that in multi-turn conversations, wrapped arguments don't
///   accumulate nested `_raw_args` layers.
pub(crate) fn sanitize_arguments(
    id: Option<&str>,
    name: &str,
    args: &str,
) -> String {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        debug!(
            tool_call_id = ?id,
            tool_name = %name,
            "tool_call arguments is empty, substituting {{}}"
        );
        return "{}".to_string();
    }

    // Fast path: if it's valid JSON, return as-is.
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return args.to_string();
    }

    // Parse attempt for round-trip unwrapping.
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(raw) = parsed.get("_raw_args").and_then(|v| v.as_str()) {
            // Already wrapped in a previous round. Unwrap to prevent nesting.
            debug!(
                tool_call_id = ?id,
                tool_name = %name,
                "unwrapping _raw_args from previous round"
            );
            // Re-sanitize the unwrapped content (may still be invalid → wrap again).
            return sanitize_arguments(id, name, raw);
        }
    }

    let wrapped = serde_json::json!({ "_raw_args": args }).to_string();
    warn!(
        tool_call_id = ?id,
        tool_name = %name,
        bad_args_len = args.len(),
        bad_args_preview = %args.chars().take(200).collect::<String>(),
        "tool_call arguments is not valid JSON, wrapped as _raw_args"
    );
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_empty_after_new() {
        let a = ToolCallAccumulator::new();
        assert!(a.is_empty());
    }

    #[test]
    fn push_and_finish_single_tool_call() {
        let mut a = ToolCallAccumulator::new();
        a.push(RawToolCallDelta {
            index: 0,
            id: Some("c1".into()),
            name: Some("get_time".into()),
            arguments: Some("{}".into()),
        });
        let v = a.finish();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "get_time");
        assert_eq!(v[0].arguments, "{}");
        assert_eq!(v[0].id.as_deref(), Some("c1"));
    }

    #[test]
    fn push_merges_fragments() {
        let mut a = ToolCallAccumulator::new();
        a.push(RawToolCallDelta {
            index: 0,
            id: Some("c1".into()),
            name: Some("foo".into()),
            arguments: None,
        });
        a.push(RawToolCallDelta {
            index: 0,
            id: None,
            name: Some("bar".into()),
            arguments: Some("{\"a\":".into()),
        });
        a.push(RawToolCallDelta {
            index: 0,
            id: None,
            name: None,
            arguments: Some("1}".into()),
        });
        let v = a.finish();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "foobar");
        assert_eq!(v[0].arguments, "{\"a\":1}");
    }

    #[test]
    fn finish_sorts_by_name() {
        let mut a = ToolCallAccumulator::new();
        a.push(RawToolCallDelta {
            index: 1,
            id: None,
            name: Some("z".into()),
            arguments: None,
        });
        a.push(RawToolCallDelta {
            index: 0,
            id: None,
            name: Some("a".into()),
            arguments: None,
        });
        let v = a.finish();
        assert_eq!(v[0].name, "a");
        assert_eq!(v[1].name, "z");
    }

    #[test]
    fn replace_from_vec_overrides() {
        let mut a = ToolCallAccumulator::new();
        a.push(RawToolCallDelta {
            index: 0,
            id: None,
            name: Some("old".into()),
            arguments: None,
        });
        a.replace_from_vec(vec![ToolCall {
            name: "new".into(),
            arguments: "{}".into(),
            id: Some("id1".into()),
        }]);
        let v = a.finish();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "new");
        assert_eq!(v[0].id.as_deref(), Some("id1"));
    }

    #[test]
    fn finish_repairs_empty_arguments_to_empty_object() {
        let mut a = ToolCallAccumulator::new();
        a.push(RawToolCallDelta {
            index: 0,
            id: Some("c1".into()),
            name: Some("noop".into()),
            arguments: Some(String::new()),
        });
        let v = a.finish();
        assert_eq!(v[0].arguments, "{}");
        assert!(serde_json::from_str::<serde_json::Value>(&v[0].arguments).is_ok());
    }

    #[test]
    fn finish_treats_whitespace_only_arguments_as_empty() {
        let mut a = ToolCallAccumulator::new();
        a.push(RawToolCallDelta {
            index: 0,
            id: Some("c1".into()),
            name: Some("noop".into()),
            arguments: Some("   \n\t  ".into()),
        });
        let v = a.finish();
        assert_eq!(v[0].arguments, "{}");
    }

    #[test]
    fn finish_wraps_malformed_arguments_with_raw_args_key() {
        let mut a = ToolCallAccumulator::new();
        a.push(RawToolCallDelta {
            index: 0,
            id: Some("c1".into()),
            name: Some("bad".into()),
            arguments: Some("query=hello".into()),
        });
        let v = a.finish();
        let parsed: serde_json::Value = serde_json::from_str(&v[0].arguments).unwrap();
        assert_eq!(parsed["_raw_args"], "query=hello");
    }

    #[test]
    fn replace_from_vec_also_sanitizes() {
        let mut a = ToolCallAccumulator::new();
        a.replace_from_vec(vec![ToolCall {
            name: "x".into(),
            arguments: "garbage".into(),
            id: Some("c1".into()),
        }]);
        let v = a.finish();
        let parsed: serde_json::Value = serde_json::from_str(&v[0].arguments).unwrap();
        assert_eq!(parsed["_raw_args"], "garbage");
    }

    #[test]
    fn sanitize_unwraps_raw_args_on_round_trip() {
        // Simulate a wrapped argument coming back from a multi-turn conversation.
        let wrapped = r#"{"_raw_args":"garbage"}"#;
        let result = sanitize_arguments(Some("c1"), "x", wrapped);
        // Should unwrap and re-wrap, not double-wrap.
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["_raw_args"], "garbage");
    }
}
