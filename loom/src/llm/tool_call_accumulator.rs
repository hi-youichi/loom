//! Accumulates tool calls from streamed deltas into complete [`ToolCall`] values.
//!
//! Used by both `ChatOpenAI` and `ChatOpenAICompat` when processing SSE
//! tool_calls delta chunks.

use std::collections::HashMap;

use crate::state::ToolCall;

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
            .map(|(id, name, arguments)| ToolCall {
                name,
                arguments,
                id: if id.is_empty() { None } else { Some(id) },
            })
            .collect();
        tool_calls.sort_by(|a, b| a.name.cmp(&b.name));
        tool_calls
    }

    /// Replace all accumulated tool calls with an externally-provided list
    /// (used by the proxy-fallback path when stream was empty but non-stream
    /// returned real tool calls).
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
}
