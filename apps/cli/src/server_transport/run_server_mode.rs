//! Server transport runner: orchestrates a CLI run against a running anureo-server.
//!
//! Entry point is [`run_server_mode`]. It:
//! 1. Parses the `--server URL` (or `ANUREO_SERVER_URL` env var) from `Args`.
//! 2. Builds a [`AnureoServerClient`] with the configured base URL and auth.
//! 3. Creates a session with anureo-server (`POST /session`).
//! 4. Sends the user prompt via the synchronous `prompt()` endpoint (v1/v2).
//!    Falls back to `prompt_async` + SSE + polling if sync prompt fails.
//! 5. Streams events to stdout (text mode) or emits NDJSON lines (JSON mode).
//! 6. Cleans up the session on exit.
//!
//! # Auth
//!
//! anureo-server supports bearer-token auth. Pass the token via the
//! `ANUREO_SERVER_AUTH` environment variable. Without this variable, the CLI
//! sends requests without an `Authorization` header (for local development).

use std::time::Duration;

use futures_util::StreamExt;
use serde_json::Value;
use tokio::time::sleep;

use cli::server_transport::{
    AnureoServerClient, PromptRequest, PromptResponse, SessionCreateRequest, SseChannelKind, SseEvent,
};

/// Default anureo-server base URL when `--server` is not provided.
const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:3030";

/// Maximum time to wait for session to reach a terminal state.
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(300);

/// Run the CLI in remote server mode.
///
/// This function is called from `main.rs` when `--server URL` is set (or
/// `ANUREO_SERVER_URL` is set). It replaces the entire in-process agent run
/// with a remote loop against the configured anureo-server instance.
///
/// # Arguments
///
/// - `args`: the parsed CLI arguments (for `--json`, `--verbose`, etc.)
/// - `server_url`: the anureo-server base URL (e.g. `"http://127.0.0.1:3030"`)
///
/// # Errors
///
/// Returns a user-facing error string on any failure. The caller
/// (`main.rs`) prints this to stderr and exits with code 1.
pub(crate) async fn run_server_mode(
    args: &crate::args::Args,
    server_url: String,
) -> Result<(), String> {
    let url = if server_url.is_empty() {
        DEFAULT_SERVER_URL.to_string()
    } else {
        server_url
    };

    // Build the HTTP client
    let mut client_builder = AnureoServerClient::builder(&url).timeout(COMPLETION_TIMEOUT);
    if let Ok(token) = std::env::var("ANUREO_SERVER_AUTH") {
        client_builder = client_builder.with_auth_token(token);
    }
    let client = client_builder
        .build()
        .map_err(|e| format!("failed to build transport: {e}"))?;

    // Build the prompt request
    let output_json = args.json || args.rest.iter().any(|arg| arg == "--json");
    let message = args
        .message
        .clone()
        .or_else(|| {
            let rest = args
                .rest
                .iter()
                .filter(|arg| arg.as_str() != "--json")
                .cloned()
                .collect::<Vec<_>>();
            if rest.is_empty() {
                None
            } else {
                Some(rest.join(" "))
            }
        })
        .ok_or_else(|| "no message provided".to_string())?;

    let prompt_req = PromptRequest::text(message);

    // Create session
    let create_req = SessionCreateRequest {
        agent: args.agent.clone(),
        title: None,
        parent_id: None,
        directory: args
            .working_folder
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
    };

    let session = client
        .create_session(&create_req)
        .await
        .map_err(|e| format!("failed to create session: {e}"))?;

    let session_id = session.id.clone();
    eprintln!("Connected to {} (session: {})", url, session_id);

    // Run the prompt with appropriate strategy
    let result = run_prompt(&client, &session_id, &prompt_req, output_json).await;

    // Best-effort abort on error
    if result.is_err() {
        let _ = client
            .interrupt(&session_id)
            .await
            .map_err(|e| tracing::debug!("interrupt failed: {e}"));
    }

    result
}

/// Run a prompt against the remote session.
///
/// Strategy:
/// 1. Try the synchronous `/prompt` endpoint first — simplest, no race conditions.
/// 2. If that returns an error that looks like "method not allowed", fall back to
///    async + SSE streaming with a polling fallback.
/// 3. SSE is subscribed BEFORE the async prompt is sent to avoid missing events.
async fn run_prompt(
    client: &AnureoServerClient,
    session_id: &str,
    prompt_req: &PromptRequest,
    output_json: bool,
) -> Result<(), String> {
    // ── Strategy 1: synchronous prompt (preferred for single-turn CLI) ────────
    match client.prompt(session_id, prompt_req).await {
        Ok(response) => {
            handle_sync_response(response, session_id, output_json)?;
            return Ok(());
        }
        Err(e) => {
            // Check if this is a "method not allowed" style error indicating
            // the server only supports async prompts
            if e.to_string().contains("405")
                || e.to_string().contains("method not allowed")
                || e.to_string().contains("not supported")
            {
                tracing::debug!("sync prompt not supported, falling back to async+SSE");
            } else {
                // Real error from the sync endpoint
                if output_json {
                    emit_json_line(serde_json::json!({
                        "type": "error",
                        "session_id": session_id,
                        "error": e.to_string(),
                    }))?;
                }
                return Err(format!("prompt failed: {e}"));
            }
        }
    }

    // ── Strategy 2: async prompt + SSE streaming ─────────────────────────────
    run_async_with_sse(client, session_id, prompt_req, output_json).await
}

fn emit_json_line(value: Value) -> Result<(), String> {
    use std::io::Write as _;

    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{value}").map_err(|e| format!("failed to write JSON output: {e}"))?;
    stdout
        .flush()
        .map_err(|e| format!("failed to flush JSON output: {e}"))
}

/// Handle the synchronous prompt response.
fn handle_sync_response(
    response: PromptResponse,
    session_id: &str,
    output_json: bool,
) -> Result<(), String> {
    let finish = response.info.finish.as_deref().unwrap_or("unknown");
    let reply = extract_reply_from_response(&response);

    // Extract a human-readable error reason from the structured error field
    // when present (set by server on HTTP 500).
    let structured_error_reason = response.error.as_ref().map(|e| e.message.clone());

    if output_json {
        let mut json_obj = serde_json::json!({
            "type": "done",
            "session_id": session_id,
            "reply": reply,
            "stop_reason": finish,
        });
        if finish == "error" {
            json_obj["error"] = serde_json::json!({
                    "message": structured_error_reason
                        .as_deref()
                        .unwrap_or("server error")
            });
        }
        emit_json_line(json_obj)?;
    } else {
        println!("{}", reply);
    }

    if finish == "error" {
        let reason = structured_error_reason.unwrap_or_else(|| "server error".to_string());
        return Err(reason);
    }

    Ok(())
}

/// Extract the reply text from a synchronous prompt response.
fn extract_reply_from_response(response: &PromptResponse) -> String {
    // Try to collect text from parts
    let texts: Vec<String> = response
        .parts
        .iter()
        .filter_map(|p| p.get("text").and_then(|t| t.as_str()).map(String::from))
        .collect();

    if !texts.is_empty() {
        return texts.join("");
    }

    // Fallback: look for text in properties
    for part in &response.parts {
        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
            if !text.is_empty() {
                return text.to_string();
            }
        }
    }

    String::new()
}

/// Run async prompt with SSE streaming and polling fallback.
///
/// Key ordering: SSE subscription is established BEFORE the async prompt is
/// sent, eliminating the race condition where server events arrive before the
/// stream is ready.
///
/// If the SSE stream closes without reaching a terminal state, falls back to
/// polling `GET /session/:id` until a terminal state is observed.
async fn run_async_with_sse(
    client: &AnureoServerClient,
    session_id: &str,
    prompt_req: &PromptRequest,
    output_json: bool,
) -> Result<(), String> {
    // ── Subscribe to SSE BEFORE sending the prompt ───────────────────────────
    let stream = client.subscribe(SseChannelKind::V2);
    tokio::pin!(stream);

    // ── Send the async prompt ────────────────────────────────────────────────
    client
        .prompt_async(session_id, prompt_req)
        .await
        .map_err(|e| format!("prompt_async failed: {e}"))?;

    let mut terminal = false;
    let mut final_reply: Option<String> = None;
    let mut finish_reason: Option<String> = None;
    let start = std::time::Instant::now();

    // ── SSE event loop ───────────────────────────────────────────────────────
    while !terminal {
        tokio::select! {
            biased;

            // Timeout guard
            _ = sleep(COMPLETION_TIMEOUT) => {
                return Err("session timed out after 5 minutes".to_string());
            }

            // SSE events
            event = stream.next() => {
                match event {
                    None => {
                        // SSE stream ended — check if we have a terminal state
                        let elapsed = start.elapsed().as_secs();
                        if final_reply.is_some() || finish_reason.is_some() {
                            // We already have a reply — SSE just closed normally
                            tracing::debug!("SSE stream closed after {}s (already have reply)", elapsed);
                            break;
                        }

                        // No reply yet — fall through to polling fallback
                        tracing::debug!("SSE stream closed after {}s with no reply, polling session state", elapsed);
                        break;
                    }
                    Some(Err(e)) => {
                        // Try to handle the error gracefully
                        let err_str = e.to_string();
                        if err_str.contains("404") || err_str.contains("not found") {
                            return Err(format!("session {} not found", session_id));
                        }
                        if err_str.contains("401") || err_str.contains("unauthorized") {
                            return Err("server requires authentication (set ANUREO_SERVER_AUTH)".to_string());
                        }
                        // Connection errors during streaming — fall to polling
                        tracing::warn!("SSE error: {e}, falling back to polling");
                        break;
                    }
                    Some(Ok(ev)) => {
                        handle_sse_event(&ev, &mut final_reply, &mut finish_reason, &mut terminal, output_json);
                    }
                }
            }
        }
    }

    // ── Polling fallback: session state check ────────────────────────────────
    // Invoked when SSE closed without a terminal state, or on SSE errors.
    // Polls the session endpoint until we see a terminal state or timeout.
    if !terminal {
        poll_session_until_terminal(
            client,
            session_id,
            &mut final_reply,
            &mut finish_reason,
            output_json,
        )
        .await?;
    }

    // ── Emit final output ────────────────────────────────────────────────────
    if output_json {
        let stop_reason = finish_reason
            .as_deref()
            .unwrap_or(if final_reply.is_some() {
                "end_turn"
            } else {
                "unknown"
            });
        let reply = final_reply.unwrap_or_default();
        let json = serde_json::json!({
            "type": "done",
            "session_id": session_id,
            "reply": reply,
            "stop_reason": stop_reason,
        });
        emit_json_line(json)?;
    } else {
        if let Some(reply) = final_reply {
            println!("{}", reply);
        }
    }

    Ok(())
}

/// Handle a single SSE event from the stream.
///
/// Updates `final_reply` and `finish_reason` when received.
/// Sets `terminal = true` when the session reaches a terminal state.
fn handle_sse_event(
    ev: &SseEvent,
    final_reply: &mut Option<String>,
    finish_reason: &mut Option<String>,
    terminal: &mut bool,
    output_json: bool,
) {
    let event_type = ev.event_type_str();

    // Skip keepalive and pure system events
    if ev.is_keepalive() {
        return;
    }

    // Print system events in JSON mode only
    if ev.is_system() {
        if output_json {
            let _ = emit_json_line(serde_json::json!({
                "type": "system",
                "event": event_type,
                "properties": ev.properties(),
            }));
        }
        return;
    }

    // ── Business events ──────────────────────────────────────────────────────
    if output_json {
        let _ = emit_json_line(serde_json::json!({
            "type": "event",
            "event": event_type,
            "properties": ev.properties(),
        }));
    }

    match event_type {
        "message.updated" | "message.created" => {
            if let Some(text) = extract_text_content(ev.properties()) {
                if text.trim().len() > 3 {
                    *final_reply = Some(text.trim().to_string());
                }
            }
        }
        "run.completed" => {
            *terminal = true;
            *finish_reason = Some("end_turn".to_string());
            if let Some(summary) = ev.properties().get("summary").and_then(|v| v.as_str()) {
                *final_reply = Some(summary.to_string());
            }
        }
        "run.failed" => {
            *terminal = true;
            let reason = ev
                .properties()
                .get("error")
                .or_else(|| ev.properties().get("reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("server error");
            *finish_reason = Some(format!("error: {}", reason));
            *final_reply = Some(format!("Server error: {}", reason));
        }
        "session.status" => {
            if let Some(status) = ev.properties().get("status").and_then(|v| v.as_str()) {
                eprintln!("[session status: {}]", status);
                if status == "completed" || status == "failed" || status == "cancelled" {
                    *terminal = true;
                    *finish_reason = Some(status.to_string());
                }
            }
        }
        "tool.call" | "tool.result" => {
            // Tool activity — emit to stderr in text mode
            if let Some(name) = ev.properties().get("name").and_then(|v| v.as_str()) {
                eprintln!("[tool: {}]", name);
            }
        }
        _ => {
            tracing::debug!(event_type = event_type, "unhandled SSE event");
        }
    }
}

/// Poll session state until a terminal state is reached.
///
/// Called as a fallback when the SSE stream closes without delivering a
/// terminal event. Checks `GET /session/:id` every 500ms until either:
/// - The session reaches a terminal state (completed/failed/cancelled/error)
/// - The global timeout is exceeded
async fn poll_session_until_terminal(
    client: &AnureoServerClient,
    session_id: &str,
    final_reply: &mut Option<String>,
    finish_reason: &mut Option<String>,
    _output_json: bool,
) -> Result<(), String> {
    const POLL_INTERVAL: Duration = Duration::from_millis(500);
    let start = std::time::Instant::now();
    let mut polls = 0usize;

    loop {
        if start.elapsed() > COMPLETION_TIMEOUT {
            return Err("session timed out during polling fallback".to_string());
        }

        sleep(POLL_INTERVAL).await;
        polls += 1;

        match client.get_session(session_id).await {
            Ok(session) => {
                tracing::debug!("poll #{}: status={:?}", polls, session.title);

                // Check for terminal state indicators in session metadata
                let status = session
                    .metadata
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("running");

                if status == "completed" {
                    *finish_reason = Some("end_turn".to_string());
                    // Try to get the summary from the session
                    if let Some(summary) = session.summary.as_ref() {
                        // Build reply from summary fields
                        if summary.files > 0 {
                            *final_reply = Some(format!(
                                "Completed: {} files changed (+{} -{})",
                                summary.files, summary.additions, summary.deletions
                            ));
                        }
                    }
                    return Ok(());
                }

                if status == "failed" || status == "error" || status == "cancelled" {
                    *finish_reason = Some(status.to_string());
                    *final_reply = Some(format!("Session {}: {}", status, session.title));
                    return Ok(());
                }

                // Also check the `finish` field on any recent message in metadata
                if let Some(finish) = session.metadata.get("finish").and_then(|v| v.as_str()) {
                    if finish == "error" {
                        *finish_reason = Some("error".to_string());
                        let err_msg = session
                            .metadata
                            .get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or("provider error");
                        *final_reply = Some(format!("Server error: {}", err_msg));
                        return Err(format!("server provider error: {}", err_msg));
                    }
                    if finish == "stop" || finish == "end_turn" {
                        *finish_reason = Some(finish.to_string());
                        return Ok(());
                    }
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                // Session gone — treat as terminal
                if err_str.contains("404") || err_str.contains("not found") {
                    if final_reply.is_none() {
                        *final_reply = Some("Session ended".to_string());
                    }
                    *finish_reason = Some("ended".to_string());
                    return Ok(());
                }
                // Transient error — log and retry
                tracing::warn!("poll error: {e}");
            }
        }
    }
}

/// Extract human-readable text from SSE event properties.
fn extract_text_content(properties: &Value) -> Option<String> {
    // Direct text field
    if let Some(text) = properties.get("text").and_then(|v| v.as_str()) {
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }

    // Direct content field (string)
    if let Some(content) = properties.get("content").and_then(|v| v.as_str()) {
        if !content.is_empty() {
            return Some(content.to_string());
        }
    }

    // Nested message.text
    if let Some(msg) = properties.get("message") {
        if let Some(text) = msg.get("text").and_then(|v| v.as_str()) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    // Assistant message parts
    if let Some(parts) = properties.get("parts").and_then(|v| v.as_array()) {
        let texts: Vec<String> = parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()).map(String::from))
            .collect();
        if !texts.is_empty() {
            return Some(texts.join(""));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_text_from_direct_field() {
        let props = json!({ "text": "Hello, world!" });
        assert_eq!(
            extract_text_content(&props),
            Some("Hello, world!".to_string())
        );
    }

    #[test]
    fn test_extract_text_from_nested_message() {
        let props = json!({
            "message": { "text": "Nested text" }
        });
        assert_eq!(
            extract_text_content(&props),
            Some("Nested text".to_string())
        );
    }

    #[test]
    fn test_extract_text_from_parts() {
        let props = json!({
            "parts": [
                { "text": "Part 1 " },
                { "text": "Part 2" }
            ]
        });
        assert_eq!(
            extract_text_content(&props),
            Some("Part 1 Part 2".to_string())
        );
    }

    #[test]
    fn test_extract_text_empty() {
        let props = json!({ "other": "data" });
        assert_eq!(extract_text_content(&props), None);
    }

    #[test]
    fn test_extract_text_from_content() {
        let props = json!({ "content": "Inline content" });
        assert_eq!(
            extract_text_content(&props),
            Some("Inline content".to_string())
        );
    }

    #[test]
    fn test_default_server_url() {
        assert_eq!(DEFAULT_SERVER_URL, "http://127.0.0.1:3030");
    }
}
