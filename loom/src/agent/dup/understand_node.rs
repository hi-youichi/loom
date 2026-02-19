//! Understand node: DUP phase 1–2, extracts structured understanding from user message.
//!
//! Reads `state.core.messages`, calls LLM with DUP prompt, parses JSON output,
//! writes `state.understood`. Optionally appends an assistant summary to `core.messages`.

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::Next;
use crate::llm::LlmClient;
use crate::message::Message;
use crate::Node;

use super::prompt::DUP_UNDERSTAND_PROMPT;
use super::state::{DupState, UnderstandOutput};

/// Understand node: extracts core goal, constraints, and context from user message.
///
/// Implements `Node<DupState>`. Reads the last user message from `state.core.messages`,
/// calls the LLM with the DUP prompt, parses JSON, and writes `state.understood`.
pub struct UnderstandNode {
    llm: Box<dyn LlmClient>,
}

impl UnderstandNode {
    /// Creates an Understand node with the given LLM client.
    pub fn new(llm: Box<dyn LlmClient>) -> Self {
        Self { llm }
    }
}

/// Tries to parse UnderstandOutput from LLM response. Supports JSON or fallback.
fn parse_understand_output(raw: &str) -> UnderstandOutput {
    // Try JSON first
    if let Ok(parsed) = serde_json::from_str::<UnderstandOutput>(raw) {
        return parsed;
    }
    // Fallback: extract from lines or use raw as relevant_context
    let mut core_goal = String::new();
    let mut key_constraints = Vec::new();
    let mut relevant_context = String::new();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("\"core_goal\"") || line.starts_with("core_goal") {
            if let Some(v) = extract_json_string_value(line) {
                core_goal = v;
            }
        } else if line.starts_with("\"key_constraints\"") || line.starts_with("key_constraints") {
            if let Some(v) = extract_json_array_value(line) {
                key_constraints = v;
            }
        } else if line.starts_with("\"relevant_context\"") || line.starts_with("relevant_context") {
            if let Some(v) = extract_json_string_value(line) {
                relevant_context = v;
            }
        }
    }

    if core_goal.is_empty() && key_constraints.is_empty() && relevant_context.is_empty() {
        relevant_context = raw.trim().to_string();
    }

    UnderstandOutput {
        core_goal,
        key_constraints,
        relevant_context,
    }
}

fn extract_json_string_value(line: &str) -> Option<String> {
    let colon = line.find(':')?;
    let rest = line[colon + 1..].trim();
    let rest = rest.trim_start_matches('"');
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_json_array_value(line: &str) -> Option<Vec<String>> {
    let start = line.find('[')?;
    let end = line.rfind(']')?;
    let inner = &line[start + 1..end];
    let items: Vec<String> = inner
        .split(',')
        .filter_map(|s| {
            let s = s.trim().trim_matches('"');
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })
        .collect();
    Some(items)
}

#[async_trait]
impl Node<DupState> for UnderstandNode {
    fn id(&self) -> &str {
        "understand"
    }

    async fn run(&self, state: DupState) -> Result<(DupState, Next), AgentError> {
        let last_user = state
            .core
            .messages
            .iter()
            .rev()
            .find_map(|m| match m {
                Message::User(s) => Some(s.as_str()),
                _ => None,
            })
            .unwrap_or("");

        let messages = vec![
            Message::system(DUP_UNDERSTAND_PROMPT),
            Message::user(last_user.to_string()),
        ];

        let response = self.llm.invoke(&messages).await?;
        let understood = parse_understand_output(response.content.trim());

        let summary = format!(
            "**Understanding**\n- Core goal: {}\n- Constraints: {:?}\n- Context: {}",
            understood.core_goal, understood.key_constraints, understood.relevant_context
        );

        let mut core = state.core;
        core.messages.push(Message::Assistant(summary));

        let new_state = DupState {
            core,
            understood: Some(understood),
        };

        Ok((new_state, Next::Continue))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_understand_output_json() {
        let json = r#"{"core_goal": "organize files", "key_constraints": ["path /tmp"], "relevant_context": "Downloads folder"}"#;
        let out = parse_understand_output(json);
        assert_eq!(out.core_goal, "organize files");
        assert_eq!(out.key_constraints, vec!["path /tmp"]);
        assert_eq!(out.relevant_context, "Downloads folder");
    }

    #[test]
    fn parse_understand_output_fallback() {
        let raw = "some raw text";
        let out = parse_understand_output(raw);
        assert!(out.core_goal.is_empty());
        assert!(out.key_constraints.is_empty());
        assert_eq!(out.relevant_context, "some raw text");
    }
}
