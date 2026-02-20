//! ThinkExpand node: one LLM call to produce 2–3 candidates (thought + tool_calls).
//!
//! Reads `TotState::core.messages`, appends ToT expand prompt, invokes LLM, parses
//! multiple candidates and writes `state.tot.candidates`. Emits `StreamEvent::TotExpand`.

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::{Next, RunContext};
use crate::message::Message;
use crate::state::ToolCall;
use crate::stream::StreamEvent;
use crate::Node;

use super::prompt::{TOT_EXPAND_SYSTEM_ADDON, TOT_RESEARCH_QUALITY_ADDON};
use super::state::{TotCandidate, TotState};

/// ThinkExpand node: produces 2–3 candidates for the next step.
///
/// Calls the LLM once with an addon that asks for multiple alternatives; parses
/// the response into `TotCandidate` (thought + tool_calls). Writes `state.tot.candidates`
/// and clears `chosen_index` / `tried_indices`. Interacts with `LlmClient`, `TotState`,
/// and `StreamEvent::TotExpand`.
pub struct ThinkExpandNode {
    /// LLM client used to generate candidate thoughts and tool_calls.
    llm: Box<dyn crate::LlmClient>,
    /// Number of candidates to request (2 or 3).
    candidates_per_step: usize,
    /// When true, append research-quality addon (multiple tool calls, step-by-step, cite sources).
    research_quality_addon: bool,
}

impl ThinkExpandNode {
    /// Creates a ThinkExpand node with the given LLM client.
    pub fn new(llm: Box<dyn crate::LlmClient>) -> Self {
        Self {
            llm,
            candidates_per_step: 3,
            research_quality_addon: false,
        }
    }

    /// Sets the number of candidates to request per step (2 or 3).
    pub fn with_candidates_per_step(mut self, n: usize) -> Self {
        self.candidates_per_step = n.min(3).max(2);
        self
    }

    /// When true, appends research-quality addon for how-to/research tasks (C1).
    pub fn with_research_quality_addon(mut self, enable: bool) -> Self {
        self.research_quality_addon = enable;
        self
    }

    /// Builds messages for the expand call: existing messages plus expand instruction.
    fn build_messages(&self, state: &TotState) -> Vec<Message> {
        let mut messages = state.core.messages.clone();
        let n = self.candidates_per_step;
        let mut addon =
            format!(
            "{}\n\nGenerate exactly {} candidates for the next step. You MUST output {} lines: {}.",
            TOT_EXPAND_SYSTEM_ADDON.trim(),
            n,
            n,
            (1..=n).map(|i| format!("CANDIDATE {}", i)).collect::<Vec<_>>().join(", ")
        );
        if self.research_quality_addon {
            addon.push_str("\n\n");
            addon.push_str(TOT_RESEARCH_QUALITY_ADDON.trim());
        }
        if let Some(Message::System(s)) = messages.first_mut() {
            *s = format!("{}\n\n{}", s, addon);
        } else {
            messages.insert(0, Message::system(addon));
        }
        messages
    }

    /// Parses LLM content into a list of TotCandidate (thought + tool_calls).
    /// Tries: (1) line-based CANDIDATE i: THOUGHT: ... | TOOL_CALLS: [...], (2) JSON block in TOOL_CALLS, (3) JSON envelope.
    fn parse_candidates(&self, content: &str) -> Vec<TotCandidate> {
        let mut out = Self::parse_candidates_line_based(content);
        if out.is_empty() {
            out = Self::parse_candidates_json_envelope(content);
        }
        if out.is_empty() {
            out.push(TotCandidate {
                thought: content.trim().to_string(),
                tool_calls: vec![],
                score: None,
            });
        }
        out.truncate(3);
        out
    }

    /// Line-based: each line "CANDIDATE N: THOUGHT: ... | TOOL_CALLS: [...]" or TOOL_CALLS: ```json ... ```
    fn parse_candidates_line_based(content: &str) -> Vec<TotCandidate> {
        let mut out = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let upper = line.to_uppercase();
            if !upper.contains("THOUGHT:") || !upper.contains("TOOL_CALLS:") {
                continue;
            }
            let thought_rest = line.splitn(2, "THOUGHT:").nth(1).unwrap_or("").trim();
            let parts: Vec<&str> = thought_rest
                .splitn(2, "TOOL_CALLS:")
                .map(str::trim)
                .collect();
            let thought = parts
                .first()
                .unwrap_or(&"")
                .replace('|', "")
                .trim()
                .to_string();
            let mut tool_calls_str = parts
                .get(1)
                .copied()
                .unwrap_or("[]")
                .trim_start_matches(':')
                .trim();
            if tool_calls_str.starts_with("```") {
                tool_calls_str = tool_calls_str.get(3..).unwrap_or("[]").trim();
                if tool_calls_str.to_lowercase().starts_with("json") {
                    tool_calls_str = tool_calls_str.get(4..).unwrap_or("[]").trim();
                }
                if let Some(close) = tool_calls_str.find("```") {
                    tool_calls_str = tool_calls_str.get(..close).unwrap_or("[]").trim();
                }
            }
            let tool_calls = Self::parse_tool_calls_json(tool_calls_str);
            out.push(TotCandidate {
                thought,
                tool_calls,
                score: None,
            });
        }
        out
    }

    /// JSON envelope: { "candidates": [ { "thought": "...", "tool_calls": [] } ] }
    fn parse_candidates_json_envelope(content: &str) -> Vec<TotCandidate> {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(content) else {
            return vec![];
        };
        let Some(arr) = v.get("candidates").and_then(|c| c.as_array()) else {
            return vec![];
        };
        let mut out = Vec::new();
        for item in arr {
            let thought = item
                .get("thought")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let tool_calls = item
                .get("tool_calls")
                .and_then(|t| t.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|o| {
                            let name = o.get("name")?.as_str()?.to_string();
                            let arguments = Self::arguments_from_value(o);
                            Some(ToolCall {
                                name,
                                arguments,
                                id: None,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            out.push(TotCandidate {
                thought,
                tool_calls,
                score: None,
            });
        }
        out
    }

    /// Extracts ToolCall.arguments string from a JSON tool-call object. When "arguments" is a
    /// JSON string (e.g. "{\"query\":\"x\"}"), use its content so Act can parse it as object.
    /// Otherwise (object or other) use JSON serialization. Avoids double-encoding that causes
    /// MCP "expected record, received string".
    fn arguments_from_value(v: &serde_json::Value) -> String {
        match v.get("arguments") {
            None => "{}".to_string(),
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
        }
    }

    fn parse_tool_calls_json(s: &str) -> Vec<ToolCall> {
        let arr: Vec<serde_json::Value> = match serde_json::from_str(s) {
            Ok(a) => a,
            Err(_) => return vec![],
        };
        arr.into_iter()
            .filter_map(|o| {
                let name = o.get("name")?.as_str()?.to_string();
                let arguments = Self::arguments_from_value(&o);
                Some(ToolCall {
                    name,
                    arguments,
                    id: None,
                })
            })
            .collect()
    }
}

#[async_trait]
impl Node<TotState> for ThinkExpandNode {
    fn id(&self) -> &str {
        "think_expand"
    }

    async fn run(&self, state: TotState) -> Result<(TotState, Next), AgentError> {
        let messages = self.build_messages(&state);
        let response = self.llm.invoke(&messages).await?;
        let mut candidates = self.parse_candidates(&response.content);
        // Fallback: when we got a single candidate with no tool_calls, use the API's
        // native tool_calls and content so the user still gets one valid path (e.g. search).
        if candidates.len() == 1
            && candidates[0].tool_calls.is_empty()
            && !response.tool_calls.is_empty()
        {
            candidates[0].tool_calls = response.tool_calls;
        }
        if candidates.len() == 1 && candidates[0].thought.is_empty() && !response.content.is_empty()
        {
            candidates[0].thought = response.content.trim().to_string();
        }
        let mut tot = state.tot;
        tot.candidates = candidates;
        tot.chosen_index = None;
        tot.tried_indices.clear();
        tot.suggest_backtrack = false;
        tot.path_failed_reason = None;
        let out = TotState {
            core: state.core,
            tot,
        };
        Ok((out, Next::Continue))
    }

    async fn run_with_context(
        &self,
        state: TotState,
        ctx: &RunContext<TotState>,
    ) -> Result<(TotState, Next), AgentError> {
        let (out, next) = self.run(state).await?;
        if let Some(ref tx) = ctx.stream_tx {
            let summaries: Vec<String> = out
                .tot
                .candidates
                .iter()
                .map(|c| c.thought.clone())
                .collect();
            let _ = tx
                .send(StreamEvent::TotExpand {
                    candidates: summaries,
                })
                .await;
        }
        Ok((out, next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::RunContext;
    use crate::llm::MockLlm;
    use crate::memory::RunnableConfig;
    use crate::state::ReActState;
    use super::super::state::TotExtension;
    use tokio::sync::mpsc;

    fn make_state() -> TotState {
        TotState {
            core: ReActState {
                messages: vec![Message::user("Search best rust formatter")],
                ..ReActState::default()
            },
            tot: TotExtension::default(),
        }
    }

    #[test]
    fn build_messages_inserts_system_when_missing() {
        let node = ThinkExpandNode::new(Box::new(MockLlm::with_no_tool_calls("ok")));
        let state = make_state();
        let messages = node.build_messages(&state);
        assert!(matches!(&messages[0], Message::System(_)));
        let sys = match &messages[0] {
            Message::System(s) => s,
            _ => unreachable!(),
        };
        assert!(sys.contains("Generate exactly 3 candidates"));
    }

    #[test]
    fn build_messages_appends_to_existing_system() {
        let node = ThinkExpandNode::new(Box::new(MockLlm::with_no_tool_calls("ok")))
            .with_candidates_per_step(2);
        let mut state = make_state();
        state
            .core
            .messages
            .insert(0, Message::system("base-system prompt"));
        let messages = node.build_messages(&state);
        let sys = match &messages[0] {
            Message::System(s) => s,
            _ => unreachable!(),
        };
        assert!(sys.contains("base-system prompt"));
        assert!(sys.contains("Generate exactly 2 candidates"));
    }

    #[test]
    fn parse_candidates_line_based_handles_json_tool_calls() {
        let content = r#"CANDIDATE 1: THOUGHT: use web search | TOOL_CALLS: [{"name":"web_search","arguments":{"query":"rust fmt"}}]
CANDIDATE 2: THOUGHT: summarize findings | TOOL_CALLS: []"#;
        let out = ThinkExpandNode::parse_candidates_line_based(content);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].thought, "use web search");
        assert_eq!(out[0].tool_calls[0].name, "web_search");
        assert_eq!(out[0].tool_calls[0].arguments, r#"{"query":"rust fmt"}"#);
        assert_eq!(out[1].thought, "summarize findings");
    }

    #[test]
    fn parse_candidates_json_envelope_works() {
        let content = r#"{
            "candidates": [
                { "thought": "collect data", "tool_calls": [{ "name": "search", "arguments": {"q":"x"} }] },
                { "thought": "summarize", "tool_calls": [] }
            ]
        }"#;
        let out = ThinkExpandNode::parse_candidates_json_envelope(content);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].thought, "collect data");
        assert_eq!(out[0].tool_calls[0].name, "search");
        assert_eq!(out[0].tool_calls[0].arguments, r#"{"q":"x"}"#);
    }

    #[test]
    fn parse_candidates_fallback_returns_single_candidate() {
        let node = ThinkExpandNode::new(Box::new(MockLlm::with_no_tool_calls("ok")));
        let out = node.parse_candidates("just think directly");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].thought, "just think directly");
        assert!(out[0].tool_calls.is_empty());
    }

    #[test]
    fn arguments_from_value_preserves_string_and_serializes_object() {
        let as_string = serde_json::json!({"arguments":"{\"q\":\"hello\"}"});
        assert_eq!(
            ThinkExpandNode::arguments_from_value(&as_string),
            r#"{"q":"hello"}"#
        );

        let as_object = serde_json::json!({"arguments":{"q":"hello"}});
        assert_eq!(
            ThinkExpandNode::arguments_from_value(&as_object),
            r#"{"q":"hello"}"#
        );
    }

    #[tokio::test]
    async fn run_uses_response_tool_calls_as_fallback() {
        let node = ThinkExpandNode::new(Box::new(MockLlm::with_get_time_call()));
        let (out, next) = node.run(make_state()).await.unwrap();
        assert!(matches!(next, Next::Continue));
        assert_eq!(out.tot.candidates.len(), 1);
        assert_eq!(out.tot.candidates[0].tool_calls.len(), 1);
        assert_eq!(out.tot.candidates[0].tool_calls[0].name, "get_time");
    }

    #[tokio::test]
    async fn run_with_context_emits_tot_expand_event() {
        let node = ThinkExpandNode::new(Box::new(MockLlm::with_no_tool_calls(
            "CANDIDATE 1: THOUGHT: alpha | TOOL_CALLS: []\nCANDIDATE 2: THOUGHT: beta | TOOL_CALLS: []",
        )));
        let (tx, mut rx) = mpsc::channel(8);
        let mut ctx = RunContext::<TotState>::new(RunnableConfig::default());
        ctx.stream_tx = Some(tx);

        let (_out, _next) = node.run_with_context(make_state(), &ctx).await.unwrap();
        match rx.recv().await {
            Some(StreamEvent::TotExpand { candidates }) => {
                assert_eq!(candidates, vec!["alpha".to_string(), "beta".to_string()]);
            }
            other => panic!("expected TotExpand event, got {:?}", other),
        }
    }
}
