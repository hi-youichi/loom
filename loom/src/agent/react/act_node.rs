//! Act node: read tool_calls, call ToolSource for each, write tool_results.
//!
//! ActNode holds a ToolSource (e.g. `Box<dyn ToolSource>`), implements `Node<ReActState>`.
//! Run sets [`ToolCallContext`](crate::tool_source::ToolCallContext) via `set_call_context`, then
//! calls `call_tool_with_context(name, args, None)` for each tool so implementations use the stored context.
//!
//! # Error Handling
//!
//! By default, tool errors propagate and short-circuit the graph. Use `with_handle_tool_errors`
//! to configure error handling:
//!
//! - `HandleToolErrors::Never` - Errors propagate (default)
//! - `HandleToolErrors::Always` - Errors are caught and returned as error messages
//! - `HandleToolErrors::Custom(handler)` - Custom error handler function
//!
//! # Streaming Support
//!
//! `ActNode` supports custom streaming through `run_with_context`. When called with
//! a `RunContext` that has `StreamMode::Custom` enabled, it creates a `ToolStreamWriter`
//! and passes it to tools via `ToolCallContext`. Tools can then emit progress updates
//! or intermediate results during execution.

use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, trace, warn};

use crate::error::AgentError;
use crate::graph::{GraphInterrupt, Interrupt, Next, Node, RunContext};
use crate::helve::{tools_requiring_approval, ApprovalPolicy, APPROVAL_REQUIRED_EVENT_TYPE};
use crate::state::{ReActState, ToolCall, ToolResult};
use crate::stream::{StreamEvent, StreamMode, ToolStreamWriter};
use crate::tool_source::{ToolCallContext, ToolSource, ToolSourceError};

/// Event type for Custom stream events emitted after each tool call (step progress).
/// Server or clients can use this to show progress (e.g. "Calling list_dir", "Done: 12 entries").
pub const STEP_PROGRESS_EVENT_TYPE: &str = "step_progress";

/// Truncates a string for logging, appending "..." if longer than max_len.
fn truncate_for_log(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max_len).collect::<String>())
    }
}

/// Parses ToolCall.arguments string to JSON Value. Logs a warning on parse failure.
fn parse_tool_arguments(arguments: &str) -> Value {
    let raw = if arguments.trim().is_empty() {
        serde_json::json!({})
    } else {
        match serde_json::from_str(arguments) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, arguments = %arguments, "tool arguments JSON parse failed, using empty object");
                serde_json::json!({})
            }
        }
    };
    if let Some(s) = raw.as_str() {
        serde_json::from_str(s).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "nested tool arguments JSON parse failed");
            raw
        })
    } else {
        raw
    }
}

/// Builds a step_progress Custom event payload for streaming.
fn step_progress_payload(tool_name: &str, call_id: &str, summary: &str) -> Value {
    serde_json::json!({
        "type": STEP_PROGRESS_EVENT_TYPE,
        "node_id": "act",
        "tool_name": tool_name,
        "call_id": call_id,
        "summary": summary,
    })
}

/// Default error message template for tool errors.
pub const DEFAULT_TOOL_ERROR_TEMPLATE: &str = "Error: {error}\n Please fix your mistakes.";

/// Default execution error message template with tool name and kwargs.
pub const DEFAULT_EXECUTION_ERROR_TEMPLATE: &str =
    "Error executing tool '{tool_name}' with kwargs {tool_kwargs} with error:\n {error}\n Please fix the error and try again.";

/// Error handler function type.
pub type ErrorHandlerFn =
    Arc<dyn Fn(&ToolSourceError, &str, &Value) -> String + Send + Sync + 'static>;

/// Configuration for how ActNode handles tool errors.
#[derive(Clone)]
pub enum HandleToolErrors {
    Never,
    Always(Option<String>),
    Custom(ErrorHandlerFn),
}

impl Default for HandleToolErrors {
    fn default() -> Self {
        Self::Never
    }
}

impl std::fmt::Debug for HandleToolErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Never => write!(f, "HandleToolErrors::Never"),
            Self::Always(msg) => write!(f, "HandleToolErrors::Always({:?})", msg),
            Self::Custom(_) => write!(f, "HandleToolErrors::Custom(<fn>)"),
        }
    }
}

fn approval_required_payload(tc: &ToolCall, args: &Value) -> Value {
    serde_json::json!({
        "type": APPROVAL_REQUIRED_EVENT_TYPE,
        "node_id": "act",
        "tool_name": tc.name,
        "call_id": tc.id,
        "arguments": args,
    })
}

/// Act node: one ReAct step that executes tool_calls and produces tool_results.
pub struct ActNode {
    tools: Box<dyn ToolSource>,
    handle_tool_errors: HandleToolErrors,
    approval_policy: Option<ApprovalPolicy>,
}

impl ActNode {
    pub fn new(tools: Box<dyn ToolSource>) -> Self {
        Self {
            tools,
            handle_tool_errors: HandleToolErrors::Never,
            approval_policy: None,
        }
    }

    pub fn with_approval_policy(mut self, policy: Option<ApprovalPolicy>) -> Self {
        self.approval_policy = policy;
        self
    }

    fn needs_approval(&self, tool_name: &str) -> bool {
        match &self.approval_policy {
            None => false,
            Some(p) => tools_requiring_approval(*p).contains(&tool_name),
        }
    }

    pub fn with_handle_tool_errors(mut self, handle_tool_errors: HandleToolErrors) -> Self {
        self.handle_tool_errors = handle_tool_errors;
        self
    }

    fn handle_error(
        &self,
        error: &ToolSourceError,
        tool_name: &str,
        tool_args: &Value,
    ) -> Option<String> {
        match &self.handle_tool_errors {
            HandleToolErrors::Never => None,
            HandleToolErrors::Always(custom_msg) => {
                let msg = custom_msg.clone().unwrap_or_else(|| {
                    DEFAULT_EXECUTION_ERROR_TEMPLATE
                        .replace("{tool_name}", tool_name)
                        .replace("{tool_kwargs}", &tool_args.to_string())
                        .replace("{error}", &error.to_string())
                });
                Some(msg)
            }
            HandleToolErrors::Custom(handler) => Some(handler(error, tool_name, tool_args)),
        }
    }
}

#[async_trait]
impl Node<ReActState> for ActNode {
    fn id(&self) -> &str {
        "act"
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        let ctx = ToolCallContext::new(state.messages.clone());
        self.tools.set_call_context(Some(ctx.clone()));
        let mut tool_results = Vec::with_capacity(state.tool_calls.len());
        let mut approval_result_consumed = false;

        for tc in &state.tool_calls {
            let args: Value = parse_tool_arguments(&tc.arguments);

            if self.needs_approval(&tc.name) {
                match state.approval_result {
                    None => {
                        let payload = approval_required_payload(tc, &args);
                        self.tools.set_call_context(None);
                        return Err(AgentError::Interrupted(GraphInterrupt(Interrupt::new(
                            payload,
                        ))));
                    }
                    Some(false) => {
                        tool_results.push(ToolResult {
                            call_id: tc.id.clone(),
                            name: Some(tc.name.clone()),
                            content: "User rejected.".to_string(),
                            is_error: true,
                        });
                        approval_result_consumed = true;
                        continue;
                    }
                    Some(true) => {
                        approval_result_consumed = true;
                    }
                }
            }

            debug!(tool = %tc.name, args = ?args, "Calling tool");

            let result = self
                .tools
                .call_tool_with_context(&tc.name, args.clone(), None)
                .await;

            match result {
                Ok(content) => {
                    trace!(
                        tool = %tc.name,
                        result_len = content.text.len(),
                        result_preview = %truncate_for_log(&content.text, 200),
                        "Tool returned"
                    );
                    tool_results.push(ToolResult {
                        call_id: tc.id.clone(),
                        name: Some(tc.name.clone()),
                        content: content.text,
                        is_error: false,
                    });
                }
                Err(e) => {
                    warn!(tool = %tc.name, error = %e, "Tool call failed");
                    if let Some(error_msg) = self.handle_error(&e, &tc.name, &args) {
                        tool_results.push(ToolResult {
                            call_id: tc.id.clone(),
                            name: Some(tc.name.clone()),
                            content: error_msg,
                            is_error: true,
                        });
                    } else {
                        self.tools.set_call_context(None);
                        return Err(AgentError::ExecutionFailed(e.to_string()));
                    }
                }
            }
        }

        self.tools.set_call_context(None);
        let new_state = ReActState {
            messages: state.messages,
            tool_calls: state.tool_calls,
            tool_results,
            turn_count: state.turn_count,
            approval_result: if approval_result_consumed {
                None
            } else {
                state.approval_result
            },
            usage: state.usage,
            total_usage: state.total_usage,
            message_count_after_last_think: state.message_count_after_last_think,
        };
        Ok((new_state, Next::Continue))
    }

    async fn run_with_context(
        &self,
        state: ReActState,
        run_ctx: &RunContext<ReActState>,
    ) -> Result<(ReActState, Next), AgentError> {
        let tool_writer = if run_ctx.stream_mode.contains(&StreamMode::Custom) {
            if let Some(tx) = &run_ctx.stream_tx {
                let tx = tx.clone();
                ToolStreamWriter::new(move |value| tx.try_send(StreamEvent::Custom(value)).is_ok())
            } else {
                ToolStreamWriter::noop()
            }
        } else {
            ToolStreamWriter::noop()
        };

        let ctx = ToolCallContext {
            recent_messages: state.messages.clone(),
            stream_writer: Some(tool_writer),
            thread_id: run_ctx.config.thread_id.clone(),
            user_id: run_ctx.config.user_id.clone(),
        };
        self.tools.set_call_context(Some(ctx.clone()));

        let mut tool_results = Vec::with_capacity(state.tool_calls.len());
        let mut approval_result_consumed = false;

        for tc in &state.tool_calls {
            let args: Value = parse_tool_arguments(&tc.arguments);

            if self.needs_approval(&tc.name) {
                match state.approval_result {
                    None => {
                        let payload = approval_required_payload(tc, &args);
                        let _ = run_ctx.emit_custom(payload.clone()).await;
                        self.tools.set_call_context(None);
                        return Err(AgentError::Interrupted(GraphInterrupt(Interrupt::new(
                            payload,
                        ))));
                    }
                    Some(false) => {
                        tool_results.push(ToolResult {
                            call_id: tc.id.clone(),
                            name: Some(tc.name.clone()),
                            content: "User rejected.".to_string(),
                            is_error: true,
                        });
                        approval_result_consumed = true;
                        let payload = step_progress_payload(
                            &tc.name,
                            tc.id.as_deref().unwrap_or(""),
                            "User rejected.",
                        );
                        let _ = run_ctx.emit_custom(payload).await;
                        continue;
                    }
                    Some(true) => {
                        approval_result_consumed = true;
                    }
                }
            }

            debug!(tool = %tc.name, args = ?args, "Calling tool");

            let result = self
                .tools
                .call_tool_with_context(&tc.name, args.clone(), None)
                .await;

            match result {
                Ok(content) => {
                    trace!(
                        tool = %tc.name,
                        result_len = content.text.len(),
                        result_preview = %truncate_for_log(&content.text, 200),
                        "Tool returned"
                    );
                    let summary = truncate_for_log(&content.text, 200);
                    tool_results.push(ToolResult {
                        call_id: tc.id.clone(),
                        name: Some(tc.name.clone()),
                        content: content.text,
                        is_error: false,
                    });
                    let call_id = tc.id.as_deref().unwrap_or("");
                    let payload = step_progress_payload(&tc.name, call_id, &summary);
                    let _ = run_ctx.emit_custom(payload).await;
                }
                Err(e) => {
                    warn!(tool = %tc.name, error = %e, "Tool call failed");
                    if let Some(error_msg) = self.handle_error(&e, &tc.name, &args) {
                        let summary = truncate_for_log(&error_msg, 200);
                        tool_results.push(ToolResult {
                            call_id: tc.id.clone(),
                            name: Some(tc.name.clone()),
                            content: error_msg,
                            is_error: true,
                        });
                        let call_id = tc.id.as_deref().unwrap_or("");
                        let payload = step_progress_payload(&tc.name, call_id, &summary);
                        let _ = run_ctx.emit_custom(payload).await;
                    } else {
                        self.tools.set_call_context(None);
                        return Err(AgentError::ExecutionFailed(e.to_string()));
                    }
                }
            }
        }

        self.tools.set_call_context(None);

        let new_state = ReActState {
            messages: state.messages,
            tool_calls: state.tool_calls,
            tool_results,
            turn_count: state.turn_count,
            approval_result: if approval_result_consumed {
                None
            } else {
                state.approval_result
            },
            usage: state.usage,
            total_usage: state.total_usage,
            message_count_after_last_think: state.message_count_after_last_think,
        };
        Ok((new_state, Next::Continue))
    }
}
