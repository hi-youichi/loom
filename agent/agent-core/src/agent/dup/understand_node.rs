//! Understand node: DUP phase 1–2, extracts structured understanding from user message.
//!
//! Reads `state.core.messages`, calls LLM with DUP prompt, parses JSON output,
//! writes `state.understood`. Optionally appends an assistant summary to `core.messages`.

use async_trait::async_trait;

use anureo_graph_core::GraphError;
use anureo_graph_core::Next;
use anureo_graph_core::Node;
use anureo_llm::message::Message;
use anureo_llm::LlmClient;

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
            if let Some(v) = extract_json_value(line) {
                core_goal = v;
            }
        } else if line.starts_with("\"key_constraints\"") || line.starts_with("key_constraints") {
            if let Some(v) = extract_json_array(line) {
                key_constraints = v;
            }
        } else if line.starts_with("\"relevant_context\"") || line.starts_with("relevant_context") {
            if let Some(v) = extract_json_value(line) {
                relevant_context = v;
            }
        }
    }

    if core_goal.is_empty() && relevant_context.is_empty() {
        relevant_context = raw.trim().to_string();
    }

    UnderstandOutput {
        core_goal,
        key_constraints,
        relevant_context,
    }
}

fn extract_json_value(line: &str) -> Option<String> {
    let colon_pos = line.find(':')?;
    let after_colon = line[colon_pos + 1..].trim();
    if let Some(rest) = after_colon.strip_prefix('"') {
        for (i, c) in rest.char_indices() {
            if c == '"' && i + 1 < rest.len() && rest.chars().nth(i + 1) == Some(',') {
                return Some(rest[..i].to_string());
            }
            if c == '"' && i + 1 == rest.len() {
                return Some(rest[..i].to_string());
            }
        }
    }
    None
}

fn extract_json_array(line: &str) -> Option<Vec<String>> {
    let colon_pos = line.find(':')?;
    let after_colon = line[colon_pos + 1..].trim();
    if !after_colon.starts_with('[') {
        return None;
    }
    let end = after_colon.rfind(']')?;
    let content = &after_colon[1..end];
    let mut results = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    for c in content.chars() {
        match c {
            '"' => in_string = !in_string,
            ',' if !in_string => {
                let trimmed = current.trim();
                if trimmed.starts_with('"') && trimmed.ends_with('"') {
                    results.push(trimmed[1..trimmed.len() - 1].to_string());
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }
    let trimmed = current.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') {
        results.push(trimmed[1..trimmed.len() - 1].to_string());
    }
    Some(results)
}

#[async_trait]
impl Node<DupState> for UnderstandNode {
    fn id(&self) -> &str {
        "understand"
    }

    async fn run(&self, mut state: DupState) -> Result<(DupState, Next), GraphError> {
        // Build messages: system prompt + last user message
        let last_user = state
            .core
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m, Message::User(_)))
            .map(|m| m.content().to_string())
            .unwrap_or_default();

        let messages = vec![
            Message::system(DUP_UNDERSTAND_PROMPT),
            Message::user(last_user.as_str()),
        ];

        let response = self.llm.invoke(&messages).await?;
        let output = parse_understand_output(&response.content);
        state.understood = Some(output.clone());

        tracing::debug!(
            "UnderstandNode: core_goal={:?}",
            output.core_goal.chars().take(50).collect::<String>()
        );

        state
            .core
            .messages
            .push(Message::assistant(&response.content));
        Ok((state, Next::Node("plan".into())))
    }
}
