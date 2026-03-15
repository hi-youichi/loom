//! Act node: read tool_calls, call ToolSource for each, write tool_results.
//!
//! ActNode holds a ToolSource (e.g. `Box<dyn ToolSource>`), implements `Node<ReActState>`.
//! Run sets [`ToolCallContext`](ToolCallContext) via `set_call_context`, then
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
use crate::llm::context_persistence;
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
                    context_persistence::save_tool_result(
                        "act",
                        None,
                        tc.id.as_deref(),
                        &tc.name,
                        &content.text,
                        false,
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
                    context_persistence::save_tool_result(
                        "act",
                        None,
                        tc.id.as_deref(),
                        &tc.name,
                        &e.to_string(),
                        true,
                    );
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
        let tools_mode = run_ctx.stream_mode.contains(&StreamMode::Tools)
            || run_ctx.stream_mode.contains(&StreamMode::Debug);

        let base_custom_writer = if run_ctx.stream_mode.contains(&StreamMode::Custom) || tools_mode
        {
            if let Some(tx) = &run_ctx.stream_tx {
                let tx = tx.clone();
                ToolStreamWriter::new(move |value| tx.try_send(StreamEvent::Custom(value)).is_ok())
            } else {
                ToolStreamWriter::noop()
            }
        } else {
            ToolStreamWriter::noop()
        };

        let mut tool_results = Vec::with_capacity(state.tool_calls.len());
        let mut approval_result_consumed = false;

        for tc in &state.tool_calls {
            let args: Value = parse_tool_arguments(&tc.arguments);

            if self.needs_approval(&tc.name) {
                match state.approval_result {
                    None => {
                        if tools_mode {
                            if let Some(tx) = &run_ctx.stream_tx {
                                let _ = tx
                                    .send(StreamEvent::ToolApproval {
                                        call_id: tc.id.clone(),
                                        name: tc.name.clone(),
                                        arguments: args.clone(),
                                    })
                                    .await;
                            }
                        } else {
                            let payload = approval_required_payload(tc, &args);
                            let _ = run_ctx.emit_custom(payload.clone()).await;
                        }
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
                        if tools_mode {
                            if let Some(tx) = &run_ctx.stream_tx {
                                let _ = tx
                                    .send(StreamEvent::ToolEnd {
                                        call_id: tc.id.clone(),
                                        name: tc.name.clone(),
                                        result: "User rejected.".to_string(),
                                        is_error: true,
                                    })
                                    .await;
                            }
                        } else {
                            let payload = step_progress_payload(
                                &tc.name,
                                tc.id.as_deref().unwrap_or(""),
                                "User rejected.",
                            );
                            let _ = run_ctx.emit_custom(payload).await;
                        }
                        continue;
                    }
                    Some(true) => {
                        approval_result_consumed = true;
                    }
                }
            }

            // Build per-tool writer with output_fn bound to this tool's call_id/name
            let per_tool_writer = if tools_mode {
                if let Some(tx) = &run_ctx.stream_tx {
                    let out_tx = tx.clone();
                    let out_call_id = tc.id.clone();
                    let out_name = tc.name.clone();
                    ToolStreamWriter::new_with_output(
                        {
                            let efn = base_custom_writer.emit_fn_clone();
                            move |v| efn(v)
                        },
                        move |content| {
                            out_tx
                                .try_send(StreamEvent::ToolOutput {
                                    call_id: out_call_id.clone(),
                                    name: out_name.clone(),
                                    content,
                                })
                                .is_ok()
                        },
                    )
                } else {
                    base_custom_writer.clone()
                }
            } else {
                base_custom_writer.clone()
            };

            let ctx = ToolCallContext {
                recent_messages: state.messages.clone(),
                stream_writer: Some(per_tool_writer),
                thread_id: run_ctx.config.thread_id.clone(),
                user_id: run_ctx.config.user_id.clone(),
                depth: run_ctx.config.depth.unwrap_or(0),
            };
            self.tools.set_call_context(Some(ctx));

            // Emit tool_start
            if tools_mode {
                if let Some(tx) = &run_ctx.stream_tx {
                    let _ = tx
                        .send(StreamEvent::ToolStart {
                            call_id: tc.id.clone(),
                            name: tc.name.clone(),
                        })
                        .await;
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
                    context_persistence::save_tool_result(
                        "act",
                        run_ctx.config.thread_id.as_deref(),
                        tc.id.as_deref(),
                        &tc.name,
                        &content.text,
                        false,
                    );
                    tool_results.push(ToolResult {
                        call_id: tc.id.clone(),
                        name: Some(tc.name.clone()),
                        content: content.text,
                        is_error: false,
                    });
                    if tools_mode {
                        if let Some(tx) = &run_ctx.stream_tx {
                            let _ = tx
                                .send(StreamEvent::ToolEnd {
                                    call_id: tc.id.clone(),
                                    name: tc.name.clone(),
                                    result: summary.to_string(),
                                    is_error: false,
                                })
                                .await;
                        }
                    } else {
                        let call_id = tc.id.as_deref().unwrap_or("");
                        let payload = step_progress_payload(&tc.name, call_id, &summary);
                        let _ = run_ctx.emit_custom(payload).await;
                    }
                }
                Err(e) => {
                    warn!(tool = %tc.name, error = %e, "Tool call failed");
                    context_persistence::save_tool_result(
                        "act",
                        run_ctx.config.thread_id.as_deref(),
                        tc.id.as_deref(),
                        &tc.name,
                        &e.to_string(),
                        true,
                    );
                    if let Some(error_msg) = self.handle_error(&e, &tc.name, &args) {
                        let summary = truncate_for_log(&error_msg, 200);
                        tool_results.push(ToolResult {
                            call_id: tc.id.clone(),
                            name: Some(tc.name.clone()),
                            content: error_msg,
                            is_error: true,
                        });
                        if tools_mode {
                            if let Some(tx) = &run_ctx.stream_tx {
                                let _ = tx
                                    .send(StreamEvent::ToolEnd {
                                        call_id: tc.id.clone(),
                                        name: tc.name.clone(),
                                        result: summary.to_string(),
                                        is_error: true,
                                    })
                                    .await;
                            }
                        } else {
                            let call_id = tc.id.as_deref().unwrap_or("");
                            let payload = step_progress_payload(&tc.name, call_id, &summary);
                            let _ = run_ctx.emit_custom(payload).await;
                        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate_for_log("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string_adds_ellipsis() {
        let long = "a".repeat(50);
        let result = truncate_for_log(&long, 10);
        assert!(result.ends_with("..."));
        assert_eq!(result.len(), 13);
    }

    #[test]
    fn parse_tool_arguments_valid_json() {
        let v = parse_tool_arguments(r#"{"path": "/tmp"}"#);
        assert_eq!(v["path"], "/tmp");
    }

    #[test]
    fn parse_tool_arguments_empty_string() {
        let v = parse_tool_arguments("");
        assert!(v.is_object());
    }

    #[test]
    fn parse_tool_arguments_whitespace_only() {
        let v = parse_tool_arguments("   ");
        assert!(v.is_object());
    }

    #[test]
    fn parse_tool_arguments_invalid_json() {
        let v = parse_tool_arguments("not json {");
        assert!(v.is_object());
    }

    #[test]
    fn parse_tool_arguments_nested_string_json() {
        let v = parse_tool_arguments(r#""{\"key\": \"val\"}""#);
        assert_eq!(v["key"], "val");
    }

    #[test]
    fn step_progress_payload_structure() {
        let p = step_progress_payload("bash", "c1", "done");
        assert_eq!(p["type"], STEP_PROGRESS_EVENT_TYPE);
        assert_eq!(p["node_id"], "act");
        assert_eq!(p["tool_name"], "bash");
        assert_eq!(p["call_id"], "c1");
        assert_eq!(p["summary"], "done");
    }

    #[test]
    fn handle_tool_errors_default_is_never() {
        let h = HandleToolErrors::default();
        assert!(matches!(h, HandleToolErrors::Never));
    }

    #[test]
    fn handle_tool_errors_debug_format() {
        assert!(format!("{:?}", HandleToolErrors::Never).contains("Never"));
        assert!(format!("{:?}", HandleToolErrors::Always(None)).contains("Always"));
        assert!(format!("{:?}", HandleToolErrors::Always(Some("msg".to_string()))).contains("msg"));
        let custom = HandleToolErrors::Custom(Arc::new(|_, _, _| "err".to_string()));
        assert!(format!("{:?}", custom).contains("Custom"));
    }

    #[test]
    fn handle_error_never_returns_none() {
        use crate::tool_source::MockToolSource;
        let node = ActNode::new(Box::new(MockToolSource::default()));
        let err = ToolSourceError::InvalidInput("test".to_string());
        assert!(node.handle_error(&err, "bash", &serde_json::json!({})).is_none());
    }

    #[test]
    fn handle_error_always_default_template() {
        use crate::tool_source::MockToolSource;
        let node = ActNode::new(Box::new(MockToolSource::default()))
            .with_handle_tool_errors(HandleToolErrors::Always(None));
        let err = ToolSourceError::InvalidInput("bad input".to_string());
        let msg = node.handle_error(&err, "bash", &serde_json::json!({"cmd": "ls"})).unwrap();
        assert!(msg.contains("bash"));
        assert!(msg.contains("bad input"));
    }

    #[test]
    fn handle_error_always_custom_message() {
        use crate::tool_source::MockToolSource;
        let node = ActNode::new(Box::new(MockToolSource::default()))
            .with_handle_tool_errors(HandleToolErrors::Always(Some("custom error".to_string())));
        let err = ToolSourceError::InvalidInput("test".to_string());
        let msg = node.handle_error(&err, "bash", &serde_json::json!({})).unwrap();
        assert_eq!(msg, "custom error");
    }

    #[test]
    fn handle_error_custom_handler() {
        use crate::tool_source::MockToolSource;
        let handler: ErrorHandlerFn = Arc::new(|e, name, _args| format!("{}: {}", name, e));
        let node = ActNode::new(Box::new(MockToolSource::default()))
            .with_handle_tool_errors(HandleToolErrors::Custom(handler));
        let err = ToolSourceError::InvalidInput("test".to_string());
        let msg = node.handle_error(&err, "bash", &serde_json::json!({})).unwrap();
        assert!(msg.contains("bash"));
    }

    #[test]
    fn approval_required_payload_structure() {
        let tc = ToolCall {
            id: Some("c1".to_string()),
            name: "delete_file".to_string(),
            arguments: "{}".to_string(),
        };
        let p = approval_required_payload(&tc, &serde_json::json!({"path": "x.txt"}));
        assert_eq!(p["type"], APPROVAL_REQUIRED_EVENT_TYPE);
        assert_eq!(p["tool_name"], "delete_file");
        assert_eq!(p["arguments"]["path"], "x.txt");
    }

    #[test]
    fn act_node_id() {
        use crate::tool_source::MockToolSource;
        let node = ActNode::new(Box::new(MockToolSource::default()));
        assert_eq!(Node::<ReActState>::id(&node), "act");
    }
}
