//! Audit-log helper methods for [`super::ChatOpenAICompat`].

use crate::support::audit::{
    build_audit_entry, LlmAuditRequest, LlmAuditRequestParams, LlmAuditResponse,
    LlmAuditToolCall, LlmAuditUsage,
};
use crate::traits::LlmResponse;

use super::ChatOpenAICompat;
use super::request::ChatCompletionRequest;

/// Per-request audit context, created once at the entry of `invoke` / `invoke_stream`.
///
/// Bundles the immutable request-scoped fields so they don't need to be
/// threaded through every helper method individually.
pub(super) struct AuditCtx<'a> {
    pub trace_id: &'a str,
    pub entry_type: &'a str,
    pub url: &'a str,
    pub start: std::time::Instant,
    pub request: LlmAuditRequest,
}

impl<'a> AuditCtx<'a> {
    fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

impl ChatOpenAICompat {
    pub(super) fn thread_id(&self) -> String {
        self.headers
            .as_ref()
            .and_then(|h| h.thread_id.as_ref())
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn build_audit_request(&self, body: &ChatCompletionRequest) -> LlmAuditRequest {
        LlmAuditRequest {
            messages: serde_json::to_value(&body.messages).unwrap_or_default(),
            tools: body
                .tools
                .as_ref()
                .map(|t| serde_json::to_value(t).unwrap_or_default()),
            parameters: LlmAuditRequestParams {
                temperature: body.temperature,
                stream: body.stream,
                tool_choice: body.tool_choice.clone(),
            },
        }
    }

    pub(super) fn build_audit_response(response: &LlmResponse) -> LlmAuditResponse {
        LlmAuditResponse {
            content: response.content.clone(),
            reasoning_content: response.reasoning_content.clone(),
            usage: response.usage.as_ref().map(|u| LlmAuditUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
            tool_calls: response
                .tool_calls
                .iter()
                .map(|tc| LlmAuditToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                })
                .collect(),
        }
    }

    /// Record a successful response.
    pub(super) fn record_success(
        &self,
        ctx: &AuditCtx<'_>,
        status: u16,
        response: LlmAuditResponse,
    ) {
        self.log_entry(ctx, status, Some(response), None);
    }

    /// Record an error and return the corresponding `LlmError`.
    pub(super) fn record_error(&self, ctx: &AuditCtx<'_>, status: u16, err_msg: String) {
        self.log_entry(ctx, status, None, Some(err_msg));
    }

    fn log_entry(
        &self,
        ctx: &AuditCtx<'_>,
        status: u16,
        response: Option<LlmAuditResponse>,
        error: Option<String>,
    ) {
        if let Some(ref log) = self.audit_log {
            let entry = build_audit_entry(
                self.thread_id(),
                ctx.trace_id.to_string(),
                ctx.entry_type,
                self.model.clone(),
                ctx.url,
                ctx.elapsed_ms(),
                status,
                ctx.request.clone(),
                response,
                error,
            );
            log.log(entry);
        }
    }
}
