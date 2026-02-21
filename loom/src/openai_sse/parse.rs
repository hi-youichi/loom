//! Parse OpenAI-style chat request into ReAct runner inputs.
//!
//! Used by HTTP handlers to build `user_message`, `system_prompt`,
//! [`RunnableConfig`](crate::memory::RunnableConfig), and optional [`HelveConfig`](crate::helve::HelveConfig) from [`ChatCompletionRequest`].

use std::path::Path;

use crate::agent::react::REACT_SYSTEM_PROMPT;
use crate::helve::{ApprovalPolicy, HelveConfig};
use crate::memory::RunnableConfig;

use super::request::ChatCompletionRequest;
use thiserror::Error;

/// Result of parsing a chat completion request for the ReAct runner.
#[derive(Debug, Clone)]
pub struct ParsedChatRequest {
    /// Last user message content (input for this turn).
    pub user_message: String,
    /// System prompt; use with `build_react_initial_state(..., system_prompt, ...)`.
    pub system_prompt: String,
    /// Config for checkpointer (thread_id etc.); use with invoke/stream.
    pub runnable_config: RunnableConfig,
    /// Whether to include usage in the final SSE chunk.
    pub include_usage: bool,
    /// When set (e.g. request had working_folder or approval_policy), use with `to_react_build_config` to build a per-request runner.
    pub helve_config: Option<HelveConfig>,
}

/// Errors while parsing a chat completion request.
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("no user message in messages")]
    NoUserMessage,
    #[error("invalid working_folder: {0}")]
    InvalidWorkingFolder(String),
    #[error("invalid approval_policy: {0}")]
    InvalidApprovalPolicy(String),
}

/// Parses approval_policy string to ApprovalPolicy. Accepts "none" | "destructive_only" | "always" (case-insensitive).
fn parse_approval_policy(s: &str) -> Result<ApprovalPolicy, ParseError> {
    match s.trim().to_lowercase().as_str() {
        "none" => Ok(ApprovalPolicy::None),
        "destructive_only" | "destructive-only" => Ok(ApprovalPolicy::DestructiveOnly),
        "always" => Ok(ApprovalPolicy::Always),
        _ => Err(ParseError::InvalidApprovalPolicy(s.to_string())),
    }
}

/// Parses an OpenAI-style request into ReAct runner inputs.
///
/// - **user_message**: Last message with `role == "user"`; its `content` (or empty string if null).
/// - **system_prompt**: First message with `role == "system"` content, or [`REACT_SYSTEM_PROMPT`].
/// - **runnable_config**: `thread_id` from request if present; otherwise default.
/// - **include_usage**: From `stream_options.include_usage` (default false).
///
/// # Errors
///
/// Returns `ParseError::NoUserMessage` if no message has `role == "user"`.
pub fn parse_chat_request(req: &ChatCompletionRequest) -> Result<ParsedChatRequest, ParseError> {
    let user_message = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role.eq_ignore_ascii_case("user"))
        .and_then(|m| m.content.as_ref().map(|c| c.as_text()))
        .unwrap_or_default();

    let has_user = req
        .messages
        .iter()
        .any(|m| m.role.eq_ignore_ascii_case("user"));
    if !has_user {
        return Err(ParseError::NoUserMessage);
    }

    let system_prompt = req
        .messages
        .iter()
        .find(|m| m.role.eq_ignore_ascii_case("system"))
        .and_then(|m| m.content.as_ref().map(|c| c.as_text()))
        .unwrap_or_else(|| REACT_SYSTEM_PROMPT.to_string());

    let runnable_config = RunnableConfig {
        thread_id: req.thread_id.clone(),
        checkpoint_id: None,
        checkpoint_ns: String::new(),
        user_id: None,
        resume_from_node_id: None,
    };

    let include_usage = req
        .stream_options
        .as_ref()
        .map(|o| o.include_usage)
        .unwrap_or(false);

    let helve_config = if req.working_folder.is_some() || req.approval_policy.is_some() {
        let working_folder = req
            .working_folder
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(ref p) = working_folder {
            let path = Path::new(p);
            if !path.exists() {
                return Err(ParseError::InvalidWorkingFolder(format!(
                    "path does not exist: {}",
                    p
                )));
            }
            if !path.is_dir() {
                return Err(ParseError::InvalidWorkingFolder(format!(
                    "not a directory: {}",
                    p
                )));
            }
        }
        let approval_policy = req
            .approval_policy
            .as_deref()
            .map(parse_approval_policy)
            .transpose()?;
        Some(HelveConfig {
            working_folder: working_folder.map(std::path::PathBuf::from),
            thread_id: req.thread_id.clone(),
            user_id: None,
            approval_policy,
            role_setting: None,
            agents_md: None,
            system_prompt_override: None,
        })
    } else {
        None
    };

    Ok(ParsedChatRequest {
        user_message,
        system_prompt,
        runnable_config,
        include_usage,
        helve_config,
    })
}
