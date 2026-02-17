//! ThinkEvaluate node: score candidates and choose the best; apply to core.
//!
//! Reads `state.tot.candidates`, assigns scores (rule-based: thought length,
//! tool_calls validity), sets `chosen_index` and writes the chosen candidate's
//! thought and tool_calls into `state.core`. Emits `StreamEvent::TotEvaluate`.

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::{Next, RunContext};
use crate::message::Message;
use crate::stream::StreamEvent;
use crate::Node;

use super::state::{TotCandidate, TotState};

/// ThinkEvaluate node: scores candidates and applies the best to core.
///
/// Rule-based scoring: thought length, tool_calls, B1 (search-keyword penalty,
/// topic-overlap bonus). Sets `state.tot.chosen_index` and writes
/// `state.core.messages` and `state.core.tool_calls`. Interacts with `TotState`, `StreamEvent::TotEvaluate`.
pub struct ThinkEvaluateNode;

/// Keywords suggesting search/research; candidates without tool_calls get a penalty.
const SEARCH_RESEARCH_KEYWORDS: &[&str] = &[
    "search",
    "find",
    "look up",
    "how to",
    "how do",
    "research",
    "what is",
    "what's",
    "why",
    "why does",
    "latest",
    "recent",
    "recommend",
];

impl ThinkEvaluateNode {
    /// Creates a ThinkEvaluate node.
    pub fn new() -> Self {
        Self
    }

    fn last_user_message(messages: &[Message]) -> Option<&str> {
        messages.iter().rev().find_map(|m| {
            if let Message::User(s) = m {
                Some(s.as_str())
            } else {
                None
            }
        })
    }

    fn has_search_research_intent(text: &str) -> bool {
        let lower = text.to_lowercase();
        SEARCH_RESEARCH_KEYWORDS.iter().any(|k| lower.contains(*k))
    }

    fn topic_overlap_bonus(user: &str, thought: &str) -> f32 {
        let user_words: std::collections::HashSet<_> = user
            .split_whitespace()
            .filter(|w| w.len() > 1)
            .map(|s| s.to_lowercase())
            .collect();
        if user_words.is_empty() {
            return 0.0;
        }
        let thought_lower = thought.to_lowercase();
        let hit = user_words
            .iter()
            .filter(|w| thought_lower.contains(w.as_str()))
            .count();
        if hit == 0 {
            0.0
        } else {
            (hit as f32 / user_words.len() as f32).min(1.0) * 0.2
        }
    }

    /// Scores one candidate (higher is better). Rule-based + B1: search penalty, topic bonus.
    fn score_candidate(c: &TotCandidate, last_user: Option<&str>) -> f32 {
        let thought_len = c.thought.trim().len();
        let thought_ok = (10..=2000).contains(&thought_len);
        let thought_score = if thought_ok { 0.5 } else { 0.2 };
        let tool_score = if c.tool_calls.is_empty() { 0.3 } else { 0.5 };
        let mut score = thought_score + tool_score;
        if let Some(user) = last_user {
            if Self::has_search_research_intent(user) && c.tool_calls.is_empty() {
                score -= 0.25;
            }
            score += Self::topic_overlap_bonus(user, &c.thought);
        }
        score
    }

    /// Picks the best candidate index and returns (index, scores).
    fn choose_best(candidates: &[TotCandidate], last_user: Option<&str>) -> (usize, Vec<f32>) {
        let scores: Vec<f32> = candidates
            .iter()
            .map(|c| Self::score_candidate(c, last_user))
            .collect();
        let chosen = scores
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);
        (chosen, scores)
    }
}

impl Default for ThinkEvaluateNode {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Node<TotState> for ThinkEvaluateNode {
    fn id(&self) -> &str {
        "think_evaluate"
    }

    async fn run(&self, state: TotState) -> Result<(TotState, Next), AgentError> {
        let mut tot = state.tot;
        if tot.candidates.is_empty() {
            tot.chosen_index = None;
            return Ok((
                TotState {
                    core: state.core,
                    tot,
                },
                Next::Continue,
            ));
        }
        let last_user = Self::last_user_message(&state.core.messages);
        let (chosen_index, scores) = Self::choose_best(&tot.candidates, last_user);
        for (c, s) in tot.candidates.iter_mut().zip(scores.iter()) {
            c.score = Some(*s);
        }
        tot.chosen_index = Some(chosen_index);
        tot.tried_indices = vec![chosen_index];

        let mut core = state.core;
        let chosen = tot.candidates.get(chosen_index).unwrap();
        core.messages
            .push(Message::Assistant(chosen.thought.clone()));
        core.tool_calls = chosen.tool_calls.clone();

        let out = TotState { core, tot };
        Ok((out, Next::Continue))
    }

    async fn run_with_context(
        &self,
        state: TotState,
        ctx: &RunContext<TotState>,
    ) -> Result<(TotState, Next), AgentError> {
        let (out, next) = self.run(state).await?;
        if let (Some(ref tx), Some(chosen)) = (ctx.stream_tx.as_ref(), out.tot.chosen_index) {
            let scores: Vec<f32> = out.tot.candidates.iter().filter_map(|c| c.score).collect();
            let _ = tx.send(StreamEvent::TotEvaluate { chosen, scores }).await;
        }
        Ok((out, next))
    }
}
