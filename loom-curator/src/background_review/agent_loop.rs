use super::memory::MemoryStore;
use super::prompts::{select_review_prompt, COMBINED_REVIEW_PROMPT};
use super::skill_registry::SkillRegistry;
use super::tools::{review_tool_specs, ReviewAction, ReviewToolExecutor};
use loom_llm::ChatOpenAICompat;
use loom_llm::LlmClient;
use loom_llm::message::{AssistantToolCall, Message};
use loom_tools::tool_source::ToolCallContent;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewMode {
    Agent,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentReviewResult {
    pub actions: Vec<ReviewAction>,
    pub summary: String,
    pub iterations: u32,
    #[serde(default)]
    pub messages: Vec<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct AgentReviewConfig {
    pub max_iterations: u32,
    pub max_session_chars: usize,
    pub mode: ReviewMode,
    pub review_memory: bool,
    pub review_skills: bool,
}

impl Default for AgentReviewConfig {
    fn default() -> Self {
        Self {
            max_iterations: 16,
            max_session_chars: 24000,
            mode: ReviewMode::Agent,
            review_memory: true,
            review_skills: true,
        }
    }
}

pub struct AgentReviewRunner;

impl AgentReviewRunner {
    pub async fn run_with_refs(
        llm: &dyn LlmClient,
        memory: &MemoryStore,
        skills: &SkillRegistry,
        session_content: &str,
        config: &AgentReviewConfig,
    ) -> Result<AgentReviewResult, String> {
        let truncated = if session_content.len() > config.max_session_chars {
            let end = session_content
                .char_indices()
                .take_while(|(i, _)| *i <= config.max_session_chars)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            &session_content[..end]
        } else {
            session_content
        };

        let prompt = Self::build_system_prompt();
        let review_prompt = select_review_prompt(config.review_memory, config.review_skills)
            .unwrap_or(COMBINED_REVIEW_PROMPT);
        let review_instruction = format!(
            "Here is the conversation to review:\n\n---\n{}\n---\n\n{}\n\nYou can only call memory and skill management tools. Other tools will be denied at runtime — do not attempt them.",
            truncated, review_prompt
        );

        let mut messages = vec![Message::system(&prompt), Message::user(review_instruction.as_str())];
        let mut executor = ReviewToolExecutor::new(memory, skills);
        let mut iterations = 0u32;
        let mut session_messages: Vec<serde_json::Value> = Vec::new();

        loop {
            if iterations >= config.max_iterations {
                info!(
                    iterations,
                    max_iterations = config.max_iterations,
                    actions_so_far = executor.actions.len(),
                    "review agent reached max iterations, requesting summary"
                );

                messages.push(Message::user(
                    "You have reached the maximum number of iterations. Summarize what you have done so far in one sentence. Do not call any more tools."
                ));
                if let Ok(final_resp) = llm.invoke(&messages).await {
                    if !final_resp.content.trim().is_empty() {
                        session_messages.push(serde_json::json!({
                            "role": "assistant",
                            "content": final_resp.content,
                            "forced_summary": true,
                        }));
                    }
                }
                break;
            }
            iterations += 1;

            let response = llm
                .invoke(&messages)
                .await
                .map_err(|e| format!("Review agent LLM call failed: {}", e))?;

            if response.tool_calls.is_empty() {
                break;
            }

            let tool_call_messages: Vec<AssistantToolCall> = response.tool_calls.iter().map(|tc| {
                AssistantToolCall {
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                    id: tc.id.clone().unwrap_or_default(),
                }
            }).collect();

            messages.push(Message::assistant_with_tool_calls(
                response.content.clone(),
                tool_call_messages.clone(),
            ));

            session_messages.push(serde_json::json!({
                "role": "assistant",
                "content": response.content,
                "tool_calls": response.tool_calls.iter().map(|tc| serde_json::json!({
                    "name": tc.name,
                    "arguments": tc.arguments,
                })).collect::<Vec<_>>(),
            }));

            for tc in &response.tool_calls {
                let args: serde_json::Value = serde_json::from_str(&tc.arguments).unwrap_or(serde_json::Value::Null);
                let result = executor.execute(&tc.name, &args);
                let result_str = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());

                info!(
                    tool = %tc.name,
                    success = result["success"].as_bool().unwrap_or(false),
                    iteration = iterations,
                    "review tool executed"
                );

                messages.push(Message::Tool {
                    tool_call_id: tc.id.clone().unwrap_or_default(),
                    content: ToolCallContent::Text(result_str),
                });

                session_messages.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tc.id,
                    "name": tc.name,
                    "result": result,
                }));
            }
        }

        let summary = Self::summarize_actions(&executor.actions);

        Ok(AgentReviewResult {
            actions: executor.actions,
            summary,
            iterations,
            messages: session_messages,
        })
    }

    fn build_system_prompt() -> String {
        "You are Loom's background review agent. Your job is to analyze conversations \
        and update two stores:\n\n\
        1. **Memory** (user preferences, project facts) — use memory_get and memory_set.\n\
        2. **Skills** (reusable task patterns) — use skills_list, skill_view, skill_create, \
        skill_edit, skill_patch, skill_write_file.\n\n\
        Rules:\n\
        - Always check existing memory/skills before writing (use _get / _list / _view first).\n\
        - Merge new info with existing content; don't duplicate.\n\
        - Skill names should be class-level (e.g. 'rust-debugging'), not session-specific.\n\
        - Use skill_patch for precise edits, skill_edit for full rewrites.\n\
        - If nothing is worth saving, respond with 'Nothing to save.' and stop.\n\
        - Do NOT call tools after producing that message.\n\n\
        Available tools: memory_get, memory_set, skills_list, skill_view, skill_create, \
        skill_edit, skill_patch, skill_delete, skill_write_file, skill_remove_file.".to_string()
    }

    pub fn summarize_actions(actions: &[ReviewAction]) -> String {
        let filtered: Vec<_> = actions
            .iter()
            .filter(|a| a.has_modification)
            .collect();
        if filtered.is_empty() {
            return "No updates.".to_string();
        }
        filtered
            .iter()
            .map(|a| a.summary.clone())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

pub fn build_review_agent_client(
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Box<dyn LlmClient> {
    let client = ChatOpenAICompat::with_config(base_url, api_key, model)
        .with_tools(review_tool_specs());
    Box::new(client)
}
