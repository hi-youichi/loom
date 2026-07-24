//! `LlmClient` trait implementation for [`super::ChatOpenAICompat`].

use async_trait::async_trait;
use tracing::{debug, trace};

use crate::error::LlmError;
use crate::support::http_retry::{
    is_retryable_reqwest_error, retry_backoff_for_attempt, TRANSIENT_HTTP_MAX_RETRIES,
};
use crate::support::thinking::{
    collect_thinking_tags, strip_thinking_tags, ThinkingSegment, ThinkingTagParser,
};
use crate::support::tool_call_accumulator::{fallback_call_id, RawToolCallDelta, ToolCallAccumulator};
use crate::support::uuid6::uuid6;
use crate::tool::ToolCall;
use crate::traits::{LlmClient, LlmResponse, LlmUsage, MessageChunk, StreamSink, ToolCallChunk};

use super::audit::AuditCtx;
use super::request::{ChatCompletionRequest, ChatCompletionResponse, ModelsResponse};
use super::retry::{
    backoff_for_attempt as compat_backoff, format_api_error_body, is_retryable_status_for,
    COMPAT_RETRY_MAX_RETRIES,
};
use super::ChatOpenAICompat;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Send a chunk to the sink, recording the first-chunk timestamp.
fn send_chunk(
    sink: &dyn StreamSink,
    chunk: MessageChunk,
    node_id: &str,
    first_chunk_at: &mut Option<std::time::Instant>,
) {
    let ts = sink.try_send_message(chunk, node_id);
    if first_chunk_at.is_none() {
        *first_chunk_at = ts;
    }
}

#[derive(Default)]
struct ToolCallStreamForwarder {
    calls: std::collections::HashMap<u32, PendingToolCall>,
}

#[derive(Default)]
struct PendingToolCall {
    call_id: String,
    name: String,
    arguments: String,
    started: bool,
}

impl ToolCallStreamForwarder {
    fn push(
        &mut self,
        index: u32,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
        sink: &dyn StreamSink,
        node_id: &str,
    ) {
        let pending = self.calls.entry(index).or_insert_with(|| PendingToolCall {
            call_id: id.clone().filter(|v| !v.is_empty()).unwrap_or_else(|| fallback_call_id(index)),
            ..Default::default()
        });
        // Keep the id chosen for the first visible event. A provider may send
        // its id after name/arguments; changing it would orphan the TUI part.
        if let Some(name) = name {
            pending.name.push_str(&name);
        }
        if let Some(ref arguments) = arguments {
            pending.arguments.push_str(&arguments);
        }

        if !pending.started && !pending.name.is_empty() {
            pending.started = true;
            let _ = sink.try_send_tool_call(
                ToolCallChunk::Started { call_id: pending.call_id.clone(), name: pending.name.clone() },
                node_id,
            );
            if !pending.arguments.is_empty() {
                let _ = sink.try_send_tool_call(
                    ToolCallChunk::Delta {
                        call_id: pending.call_id.clone(),
                        arguments_delta: pending.arguments.clone(),
                    },
                    node_id,
                );
            }
        } else if pending.started {
            // The arguments carried by this delta have not previously been
            // forwarded. Name fragments are intentionally not resent: the
            // OpenCode protocol has no tool-name delta event.
            if let Some(arguments) = arguments {
                let _ = sink.try_send_tool_call(
                    ToolCallChunk::Delta { call_id: pending.call_id.clone(), arguments_delta: arguments },
                    node_id,
                );
            }
        }
    }

    fn finish(self, sink: &dyn StreamSink, node_id: &str) {
        for pending in self.calls.into_values().filter(|pending| pending.started) {
            let _ = sink.try_send_tool_call(
                ToolCallChunk::Ended { call_id: pending.call_id, arguments: pending.arguments },
                node_id,
            );
        }
    }
}

impl ChatOpenAICompat {
    /// Send a POST request with transport-level retries only.
    async fn send_post(
        &self,
        url: &str,
        body: &ChatCompletionRequest,
        request_id: &str,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let mut attempt = 0;
        loop {
            match self
                .add_headers_to_request(
                    self.client.post(url).bearer_auth(&self.api_key).json(body),
                    request_id,
                )
                .send()
                .await
            {
                Ok(res) => return Ok(res),
                Err(e)
                    if is_retryable_reqwest_error(&e) && attempt < TRANSIENT_HTTP_MAX_RETRIES =>
                {
                    let delay = retry_backoff_for_attempt(attempt);
                    tracing::warn!(
                        url = %url,
                        attempt = attempt + 1,
                        max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                        delay_secs = delay.as_secs_f64(),
                        error = ?e,
                        "transport error, retrying"
                    );
                    attempt += 1;
                    tokio::time::sleep(delay).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Send request with full retry (transport errors + retryable status codes).
    ///
    /// Returns the successful `reqwest::Response`. The caller is responsible
    /// for reading the body (bytes for `invoke`, stream for `invoke_stream`).
    async fn send_with_retry(
        &self,
        url: &str,
        body: &ChatCompletionRequest,
        request_id: &str,
        log_prefix: &str,
        ctx: &AuditCtx<'_>,
    ) -> Result<reqwest::Response, LlmError> {
        let trace_id = ctx.trace_id;
        // --- Phase 1: initial send with transport retry ---
        let res = match self.send_post(url, body, request_id).await {
            Ok(res) => res,
            Err(e) => {
                let msg = format!("{log_prefix} request failed: {e} (trace_id: {trace_id})");
                return Err(self.audit_error(ctx, 0, msg));
            }
        };

        let status = res.status();
        if status.is_success() {
            return Ok(res);
        }

        // --- Phase 2: read error body, classify ---
        let body_bytes = res.bytes().await.unwrap_or_default();
        let error_msg = format_api_error_body(&body_bytes);

        if !is_retryable_status_for(status, &self.base_url, &error_msg) {
            let msg =
                format!("{log_prefix} API error {status}: {error_msg} (trace_id: {trace_id})");
            return Err(self.audit_error(ctx, status.as_u16(), msg));
        }

        // --- Phase 3: retry on retryable status ---
        for attempt in 0..COMPAT_RETRY_MAX_RETRIES {
            let delay = compat_backoff(attempt);
            tracing::warn!(
                status = %status,
                attempt = attempt + 1,
                max_retries = COMPAT_RETRY_MAX_RETRIES,
                delay_secs = delay.as_secs_f64(),
                "{log_prefix} retryable status, retrying"
            );
            tokio::time::sleep(delay).await;

            let retry_res = match self.send_post(url, body, request_id).await {
                Ok(res) => res,
                Err(e)
                    if is_retryable_reqwest_error(&e) && attempt < COMPAT_RETRY_MAX_RETRIES - 1 =>
                {
                    tracing::warn!(
                        url = %url,
                        attempt = attempt + 1,
                        error = ?e,
                        "{log_prefix} retry send failed, retrying"
                    );
                    continue;
                }
                Err(e) => {
                    let msg = format!("{log_prefix} request failed: {e} (trace_id: {trace_id})");
                    return Err(self.audit_error(ctx, 0, msg));
                }
            };

            let retry_status = retry_res.status();
            if retry_status.is_success() {
                return Ok(retry_res);
            }

            let retry_bytes = retry_res.bytes().await.unwrap_or_default();
            let retry_msg = format_api_error_body(&retry_bytes);

            if !is_retryable_status_for(retry_status, &self.base_url, &retry_msg) {
                let msg = format!(
                    "{log_prefix} API error {retry_status}: {retry_msg} (trace_id: {trace_id})"
                );
                return Err(self.audit_error(ctx, retry_status.as_u16(), msg));
            }

            if attempt == COMPAT_RETRY_MAX_RETRIES - 1 {
                let msg = format!(
                    "{log_prefix} API error {retry_status}: {retry_msg} \
                     (trace_id: {trace_id}) (after {COMPAT_RETRY_MAX_RETRIES} retries)"
                );
                return Err(self.audit_error(ctx, retry_status.as_u16(), msg));
            }
        }

        unreachable!("retry loop always returns on last iteration")
    }

    /// Record an audit error entry and return the corresponding `LlmError`.
    fn audit_error(&self, ctx: &AuditCtx<'_>, status: u16, err_msg: String) -> LlmError {
        self.record_error(ctx, status, err_msg.clone());
        LlmError::InvokeFailed(err_msg)
    }
}

// ---------------------------------------------------------------------------
// LlmClient impl
// ---------------------------------------------------------------------------

#[async_trait]
impl LlmClient for ChatOpenAICompat {
    async fn invoke(&self, messages: &[crate::message::Message]) -> Result<LlmResponse, LlmError> {
        let trace_id = uuid6().to_string();
        let request_id = uuid6().to_string();
        let url = self.chat_completions_url();
        let body = self.build_request(messages, false);
        let tools_count = self.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        let ctx = AuditCtx {
            trace_id: &trace_id,
            entry_type: "chat",
            url: &url,
            start: std::time::Instant::now(),
            request: self.build_audit_request(&body),
        };
        debug!(
            trace_id = %trace_id,
            request_id = %request_id,
            url = %url,
            model = %self.model,
            message_count = messages.len(),
            tools_count = tools_count,
            "OpenAI-compat chat create"
        );

        let res = self
            .send_with_retry(&url, &body, &request_id, "OpenAI-compat", &ctx)
            .await?;

        let body_bytes = res.bytes().await.map_err(|e| {
            let msg = format!("OpenAI-compat response read: {e} (trace_id: {trace_id})");
            self.audit_error(&ctx, 0, msg)
        })?;

        let response: ChatCompletionResponse =
            serde_json::from_slice(&body_bytes).map_err(|e| {
                LlmError::InvokeFailed(format!(
                    "OpenAI-compat response parse: {e} (trace_id: {trace_id})"
                ))
            })?;

        let choice = response.choices.into_iter().next().ok_or_else(|| {
            LlmError::InvokeFailed(format!(
                "OpenAI-compat returned no choices (trace_id: {trace_id})"
            ))
        })?;

        let msg = choice.message;
        let finish_reason = choice.finish_reason;
        let content = msg.content.unwrap_or_default();
        let reasoning_content = msg
            .reasoning_content
            .or_else(|| collect_thinking_tags(&content));
        let content = if self.parse_thinking_tags {
            strip_thinking_tags(&content)
        } else {
            content
        };
        let tool_calls: Vec<ToolCall> = msg
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .filter_map(|tc| {
                tc.function.as_ref().map(|f| ToolCall {
                    name: f.name.clone(),
                    arguments: f.arguments.clone(),
                    id: tc.id.clone(),
                })
            })
            .collect();

        let usage = response.usage.map(|u| LlmUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
            prompt_tokens_details: u.prompt_tokens_details,
            completion_tokens_details: u.completion_tokens_details,
        });

        let llm_response = LlmResponse {
            content,
            reasoning_content,
            tool_calls,
            usage,
            finish_reason,
            ..Default::default()
        };
        let audit_response = Self::build_audit_response(&llm_response);
        self.record_success(&ctx, 200, audit_response);
        Ok(llm_response)
    }

    async fn invoke_stream(
        &self,
        messages: &[crate::message::Message],
        sink: Option<&dyn StreamSink>,
        node_id: &str,
    ) -> Result<LlmResponse, LlmError> {
        if sink.is_none() {
            return self.invoke(messages).await;
        }

        let trace_id = uuid6().to_string();
        let request_id = uuid6().to_string();
        let sink = sink.expect("sink must be Some when streaming");
        let url = self.chat_completions_url();
        let body = self.build_request(messages, true);
        let tools_count = self.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        let ctx = AuditCtx {
            trace_id: &trace_id,
            entry_type: "chat_stream",
            url: &url,
            start: std::time::Instant::now(),
            request: self.build_audit_request(&body),
        };
        debug!(
            trace_id = %trace_id,
            request_id = %request_id,
            url = %url,
            model = %self.model,
            message_count = messages.len(),
            stream = true,
            tools_count = tools_count,
            "OpenAI-compat chat create_stream"
        );

        let mut res = self
            .send_with_retry(&url, &body, &request_id, "OpenAI-compat stream", &ctx)
            .await?;

        // ---- SSE body reading ----
        let mut buf = Vec::<u8>::new();
        let mut full_content = String::new();
        let mut full_reasoning_content = String::new();
        let mut sent_any_content = false;
        let mut tool_calls_acc = ToolCallAccumulator::new();
        let mut tool_calls_forwarder = ToolCallStreamForwarder::default();
        let mut stream_usage: Option<LlmUsage> = None;
        let mut thinking_parser = self.parse_thinking_tags.then(ThinkingTagParser::new);
        let mut first_chunk_at: Option<std::time::Instant> = None;
        let mut stream_finish_reason: Option<String> = None;

        'sse: loop {
            let bytes = match res.chunk().await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => break 'sse,
                Err(e) => {
                    let msg = format!("OpenAI-compat stream body: {e}");
                    return Err(self.audit_error(&ctx, 0, msg));
                }
            };

            buf.extend_from_slice(&bytes);

            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
                let line = match std::str::from_utf8(&line_bytes) {
                    Ok(s) => s.trim(),
                    Err(_) => continue,
                };
                if line.is_empty() || !line.starts_with("data: ") {
                    continue;
                }
                let data = line.trim_start_matches("data: ").trim();
                if data == "[DONE]" {
                    break 'sse;
                }
                let mut stream_chunk: super::stream::StreamChunk = match serde_json::from_str(data)
                {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                if let Some(u) = stream_chunk.usage.take() {
                    stream_usage = Some(LlmUsage {
                        prompt_tokens: u.prompt_tokens,
                        completion_tokens: u.completion_tokens,
                        total_tokens: u.total_tokens,
                        prompt_tokens_details: u.prompt_tokens_details,
                        completion_tokens_details: u.completion_tokens_details,
                    });
                }

                let choices = match stream_chunk.choices {
                    Some(c) => c,
                    None => continue,
                };

                for choice in choices {
                    let mut delta = choice.delta;

                    if let Some(fr) = choice.finish_reason {
                        if stream_finish_reason.is_none() {
                            stream_finish_reason = Some(fr);
                        }
                    }

                    // Move fields out of delta to avoid cloning — delta is
                    // consumed by the for loop, so we own it.
                    if let Some(reasoning_content) = delta.reasoning_content.take() {
                        if !reasoning_content.is_empty() {
                            full_reasoning_content.push_str(&reasoning_content);
                            // Move the owned String into MessageChunk — no clone needed.
                            send_chunk(
                                sink,
                                MessageChunk::thinking(reasoning_content),
                                node_id,
                                &mut first_chunk_at,
                            );
                        }
                    }

                    if let Some(content) = delta.content.take() {
                        if !content.is_empty() {
                            full_content.push_str(&content);
                            sent_any_content = true;

                            if let Some(ref mut parser) = thinking_parser {
                                for seg in parser.feed(&content) {
                                    let chunk = match seg {
                                        ThinkingSegment::Message(s) => MessageChunk::message(s),
                                        ThinkingSegment::Thinking(s) => MessageChunk::thinking(s),
                                    };
                                    send_chunk(sink, chunk, node_id, &mut first_chunk_at);
                                }
                            } else {
                                // Move the owned String into MessageChunk — no clone needed.
                                send_chunk(
                                    sink,
                                    MessageChunk::message(content),
                                    node_id,
                                    &mut first_chunk_at,
                                );
                            }
                        }
                    }

                    if let Some(tool_calls) = delta.tool_calls.take() {
                        for tc in tool_calls {
                            // Destructure function once to avoid double-move
                            let (name, arguments) = match tc.function {
                                Some(f) => (f.name, f.arguments),
                                None => (None, None),
                            };
                            tool_calls_forwarder.push(
                                tc.index,
                                tc.id.clone(),
                                name.clone(),
                                arguments.clone(),
                                sink,
                                node_id,
                            );
                            tool_calls_acc.push(RawToolCallDelta {
                                index: tc.index,
                                id: tc.id,
                                name,
                                arguments,
                            });
                        }
                    }
                }
            }
        }

        // Flush remaining thinking-parser state
        if let Some(parser) = thinking_parser {
            if let Some(seg) = parser.flush() {
                let chunk = match seg {
                    ThinkingSegment::Message(s) => MessageChunk::message(s),
                    ThinkingSegment::Thinking(s) => MessageChunk::thinking(s),
                };
                send_chunk(sink, chunk, node_id, &mut first_chunk_at);
            }
        }

        // Fallback: if stream produced nothing but tokens were consumed, try non-streaming
        let completion_tokens = stream_usage
            .as_ref()
            .map(|u| u.completion_tokens)
            .unwrap_or(0);
        if full_content.is_empty()
            && full_reasoning_content.is_empty()
            && tool_calls_acc.is_empty()
            && completion_tokens > 0
        {
            tracing::warn!(
                trace_id = %trace_id,
                completion_tokens,
                "Stream returned empty content with non-zero tokens; \
                 falling back to non-streaming request"
            );
            if let Ok(fallback_resp) = self.invoke(messages).await {
                if !fallback_resp.content.is_empty()
                    || fallback_resp.reasoning_content.is_some()
                    || !fallback_resp.tool_calls.is_empty()
                {
                    full_content = fallback_resp.content.clone();
                    if let Some(reasoning_content) = fallback_resp.reasoning_content.clone() {
                        full_reasoning_content = reasoning_content.clone();
                        send_chunk(
                            sink,
                            MessageChunk::thinking(reasoning_content),
                            node_id,
                            &mut first_chunk_at,
                        );
                    }
                    if !full_content.is_empty() {
                        sent_any_content = true;
                        send_chunk(
                            sink,
                            MessageChunk::message(full_content.clone()),
                            node_id,
                            &mut first_chunk_at,
                        );
                    }
                    if stream_usage.is_none() {
                        stream_usage = fallback_resp.usage;
                    }
                    tool_calls_acc.replace_from_vec(fallback_resp.tool_calls);
                }
            }
        }

        if !sent_any_content && !full_content.is_empty() {
            send_chunk(
                sink,
                MessageChunk::message(full_content.clone()),
                node_id,
                &mut first_chunk_at,
            );
        }

        tool_calls_forwarder.finish(sink, node_id);

        let tool_calls = tool_calls_acc.finish();

        trace!(
            trace_id = %trace_id,
            url = %url,
            model = %self.model,
            reasoning_len = full_reasoning_content.len(),
            content_len = full_content.len(),
            tool_calls = ?tool_calls.len(),
            usage = ?stream_usage,
            "OpenAI-compat stream response"
        );

        let reasoning_content = if full_reasoning_content.is_empty() {
            collect_thinking_tags(&full_content)
        } else {
            Some(full_reasoning_content)
        };

        let response = LlmResponse {
            content: if self.parse_thinking_tags {
                strip_thinking_tags(&full_content)
            } else {
                full_content
            },
            reasoning_content,
            tool_calls,
            usage: stream_usage,
            first_chunk_at,
            finish_reason: stream_finish_reason,
        };
        let audit_response = Self::build_audit_response(&response);
        self.record_success(&ctx, 200, audit_response);
        Ok(response)
    }

    async fn list_models(&self) -> Result<Vec<crate::traits::ModelInfo>, LlmError> {
        let request_id = uuid6().to_string();
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{base}/models");
        let res = self
            .add_headers_to_request(
                self.client.get(&url).bearer_auth(&self.api_key),
                &request_id,
            )
            .send()
            .await
            .map_err(|e| LlmError::InvokeFailed(format!("list_models request failed: {e}")))?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(LlmError::InvokeFailed(format!(
                "list_models failed: {status} - {body}"
            )));
        }

        let body = res
            .text()
            .await
            .map_err(|e| LlmError::InvokeFailed(format!("list_models read body failed: {e}")))?;

        let models_resp: ModelsResponse = serde_json::from_str(&body)
            .map_err(|e| LlmError::InvokeFailed(format!("list_models parse failed: {e}")))?;

        Ok(models_resp
            .data
            .into_iter()
            .map(|m| crate::traits::ModelInfo {
                id: m.id,
                created: m.created,
                owned_by: m.owned_by,
            })
            .collect())
    }
}
