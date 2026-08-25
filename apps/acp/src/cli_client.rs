//! CLI-facing ACP client.
//!
//! `anureo acp` is the IDE stdio bridge. This module is the other direction:
//! the normal `anureo --acp` command acts as an ACP client and talks directly to
//! the server WebSocket endpoint.

use std::path::PathBuf;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct CliAcpOptions {
    pub url: String,
    pub cwd: PathBuf,
    pub session_id: Option<String>,
    pub message: Option<String>,
    pub json_output: bool,
    pub pretty: bool,
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub async fn run(options: CliAcpOptions) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut ws, response) = tokio::time::timeout(
        REQUEST_TIMEOUT,
        tokio_tungstenite::connect_async(&options.url),
    )
    .await
    .map_err(|_| format!("ACP WebSocket connection timed out: {}", options.url))??;

    tracing::debug!(status = %response.status(), url = %options.url, "ACP CLI connected");

    let initialize = request(
        &mut ws,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientInfo": {"name": "anureo-cli", "version": env!("CARGO_PKG_VERSION")},
            "clientCapabilities": {}
        }),
        &options,
    )
    .await?;
    emit_response("initialize", &initialize, &options)?;

    let session_method = if options.session_id.is_some() {
        "session/load"
    } else {
        "session/new"
    };
    let session_params = if let Some(session_id) = &options.session_id {
        json!({
            "sessionId": session_id,
            "cwd": options.cwd,
            "mcpServers": []
        })
    } else {
        json!({
            "cwd": options.cwd,
            "mcpServers": []
        })
    };
    let session = request(&mut ws, session_method, session_params, &options).await?;
    emit_response(session_method, &session, &options)?;

    let session_id = options.session_id.clone().or_else(|| {
        session
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });

    if let Some(message) = options.message.as_deref().filter(|m| !m.trim().is_empty()) {
        let session_id = session_id.ok_or("ACP session response did not contain sessionId")?;
        let prompt = prompt_request(
            &mut ws,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": message}]
            }),
            &options,
        )
        .await?;
        emit_response("session/prompt", &prompt, &options)?;
        if !options.json_output {
            println!();
        }
    }

    let _ = ws.close(None).await;
    Ok(())
}

async fn prompt_request(
    ws: &mut Ws,
    method: &str,
    params: Value,
    options: &CliAcpOptions,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let id = next_request_id();
    ws.send(Message::Text(
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string(),
    ))
    .await?;

    let mut cancel_signal = Box::pin(tokio::signal::ctrl_c());
    let mut cancel_sent = false;
    let wait = async {
        loop {
            tokio::select! {
                _ = &mut cancel_signal, if !cancel_sent => {
                    ws.send(Message::Text(
                        json!({
                            "jsonrpc": "2.0",
                            "method": "session/cancel",
                            "params": {"sessionId": params["sessionId"]}
                        }).to_string(),
                    )).await?;
                    cancel_sent = true;
                    eprintln!("ACP prompt cancellation requested");
                }
                next = ws.next() => {
                    let message = next.ok_or("ACP WebSocket closed before prompt response")??;
                    let Message::Text(text) = message else { continue; };
                    let value: Value = serde_json::from_str(&text)?;

                    if value.get("id").and_then(Value::as_u64) == Some(id) {
                        if let Some(error) = value.get("error") {
                            return Err(format!("ACP {method} failed: {error}").into());
                        }
                        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
                    }

                    if value.get("method").and_then(Value::as_str).is_some() {
                        emit_notification(&value, options)?;
                    }
                }
            }
        }
    };

    tokio::time::timeout(REQUEST_TIMEOUT, wait)
        .await
        .map_err(|_| format!("ACP request timed out: {method}"))?
}

async fn request(
    ws: &mut Ws,
    method: &str,
    params: Value,
    options: &CliAcpOptions,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let id = next_request_id();
    ws.send(Message::Text(
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string(),
    ))
    .await?;

    loop {
        let message = tokio::time::timeout(REQUEST_TIMEOUT, ws.next())
            .await
            .map_err(|_| format!("ACP request timed out: {method}"))?
            .ok_or("ACP WebSocket closed before response")??;

        let Message::Text(text) = message else {
            continue;
        };
        let value: Value = serde_json::from_str(&text)?;

        if value.get("id").and_then(Value::as_u64) == Some(id) {
            if let Some(error) = value.get("error") {
                return Err(format!("ACP {method} failed: {error}").into());
            }
            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }

        if value.get("method").and_then(Value::as_str).is_some() {
            emit_notification(&value, options)?;
        }
    }
}

fn emit_response(
    method: &str,
    result: &Value,
    options: &CliAcpOptions,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if options.json_output {
        let frame = json!({"type": "response", "method": method, "result": result});
        if options.pretty {
            println!("{}", serde_json::to_string_pretty(&frame)?);
        } else {
            println!("{}", serde_json::to_string(&frame)?);
        }
    } else if method == "session/new" || method == "session/load" {
        if let Some(session_id) = result.get("sessionId").and_then(Value::as_str) {
            eprintln!("ACP session: {session_id}");
        }
    }
    Ok(())
}

fn emit_notification(
    value: &Value,
    options: &CliAcpOptions,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if options.json_output {
        if options.pretty {
            println!("{}", serde_json::to_string_pretty(value)?);
        } else {
            println!("{}", serde_json::to_string(value)?);
        }
        return Ok(());
    }

    if value.get("method").and_then(Value::as_str) == Some("session/update") {
        let update = value.pointer("/params/update").unwrap_or(&Value::Null);
        if update.get("sessionUpdate").and_then(Value::as_str) == Some("agent_message_chunk") {
            if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                print!("{text}");
            }
        }
    }
    Ok(())
}

fn next_request_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}
