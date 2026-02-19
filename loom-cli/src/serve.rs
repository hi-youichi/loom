//! WebSocket server for CLI remote mode.
//!
//! Listens on ws://127.0.0.1:8080, handles run, tools_list, tool_show, ping.

use futures_util::{SinkExt, StreamExt};
use loom::{
    build_helve_config, build_react_run_context, run_agent, AnyStreamEvent, AgentType, ClientRequest,
    ErrorResponse, RunCmd, RunEndResponse, RunOptions, RunStreamEventResponse, ServerResponse,
    ToolShowOutput, ToolShowResponse, ToolsListResponse,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tracing::info;

const DEFAULT_WS_ADDR: &str = "127.0.0.1:8080";

/// Runs the WebSocket server. Listens on `addr` (default 127.0.0.1:8080).
pub async fn run_serve(addr: Option<&str>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = addr.unwrap_or(DEFAULT_WS_ADDR);
    let listener = TcpListener::bind(addr).await?;
    info!("WebSocket server listening on ws://{}", addr);

    while let Ok((stream, peer)) = listener.accept().await {
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream).await {
                tracing::warn!("connection {} error: {}", peer, e);
            }
        });
    }
    Ok(())
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_stream = accept_async(stream).await?;
    let (mut write, mut read) = ws_stream.split();

    while let Some(res) = read.next().await {
        let msg = match res {
            Ok(m) => m,
            Err(_) => break,
        };
        if !msg.is_text() && !msg.is_binary() {
            continue;
        }
        let text = msg.to_text().unwrap_or("");
        if let Err(e) = handle_request_and_send(text, &mut write).await {
            tracing::warn!("handle_request error: {}", e);
            break;
        }
    }
    Ok(())
}

async fn handle_request_and_send<W>(
    text: &str,
    write: &mut W,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    W: SinkExt<tokio_tungstenite::tungstenite::Message> + Unpin,
    W::Error: std::error::Error + Send + Sync + 'static,
{
    let req: ClientRequest = match serde_json::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            let resp = ServerResponse::Error(ErrorResponse {
                id: None,
                error: format!("parse error: {}", e),
            });
            send_response(write, &resp).await?;
            return Ok(());
        }
    };

    match req {
        ClientRequest::Run(r) => {
            if let Some(resp) = handle_run(r, write).await? {
                send_response(write, &resp).await?;
            }
        }
        ClientRequest::ToolsList(r) => {
            send_response(write, &handle_tools_list(r).await).await?;
        }
        ClientRequest::ToolShow(r) => {
            send_response(write, &handle_tool_show(r).await).await?;
        }
        ClientRequest::Ping(r) => {
            send_response(write, &ServerResponse::Pong(loom::PongResponse { id: r.id }))
                .await?;
        }
    }
    Ok(())
}

async fn send_response<W>(
    write: &mut W,
    response: &ServerResponse,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    W: SinkExt<tokio_tungstenite::tungstenite::Message> + Unpin,
    W::Error: std::error::Error + Send + Sync + 'static,
{
    let json = serde_json::to_string(response).unwrap_or_else(|_| {
        serde_json::to_string(&ServerResponse::Error(ErrorResponse {
            id: None,
            error: "serialization error".to_string(),
        }))
        .unwrap()
    });
    write
        .send(tokio_tungstenite::tungstenite::Message::Text(json))
        .await?;
    Ok(())
}

/// Returns Some(response) when a single response should be sent by the caller; None when we already sent (streaming case).
async fn handle_run<W>(
    r: loom::RunRequest,
    write: &mut W,
) -> Result<Option<ServerResponse>, Box<dyn std::error::Error + Send + Sync>>
where
    W: SinkExt<tokio_tungstenite::tungstenite::Message> + Unpin,
    W::Error: std::error::Error + Send + Sync + 'static,
{
    let id = r.id.clone();
    let output_json = r.output_json == Some(true);
    let opts = RunOptions {
        message: r.message,
        working_folder: r.working_folder.map(PathBuf::from),
        thread_id: r.thread_id,
        verbose: r.verbose.unwrap_or(false),
        got_adaptive: r.got_adaptive.unwrap_or(false),
        display_max_len: 2000,
        output_json,
    };
    let cmd = match r.agent {
        AgentType::React => RunCmd::React,
        AgentType::Dup => RunCmd::Dup,
        AgentType::Tot => RunCmd::Tot,
        AgentType::Got => RunCmd::Got {
            got_adaptive: opts.got_adaptive,
        },
    };

    if output_json {
        let events: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let on_event = Box::new(move |ev: AnyStreamEvent| {
            if let Ok(v) = ev.to_format_a() {
                if let Ok(mut vec) = events_clone.lock() {
                    vec.push(v);
                }
            }
        });
        let result = run_agent(&opts, &cmd, Some(on_event)).await;
        let events = events.lock().map(|v| v.clone()).unwrap_or_default();
        match result {
            Ok(reply) => {
                for event in events {
                    send_response(write, &ServerResponse::RunStreamEvent(RunStreamEventResponse { id: id.clone(), event })).await?;
                }
                send_response(write, &ServerResponse::RunEnd(RunEndResponse {
                    id,
                    reply,
                    usage: None,
                    total_usage: None,
                }))
                .await?;
            }
            Err(e) => {
                send_response(write, &ServerResponse::Error(ErrorResponse {
                    id: Some(id),
                    error: e.to_string(),
                }))
                .await?;
            }
        }
        return Ok(None);
    }

    let result = run_agent(&opts, &cmd, None).await;
    Ok(Some(match result {
        Ok(reply) => ServerResponse::RunEnd(RunEndResponse {
            id,
            reply,
            usage: None,
            total_usage: None,
        }),
        Err(e) => ServerResponse::Error(ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }))
}

async fn handle_tools_list(r: loom::ToolsListRequest) -> ServerResponse {
    let id = r.id.clone();
    let opts = RunOptions {
        message: String::new(),
        working_folder: None,
        thread_id: None,
        verbose: false,
        got_adaptive: false,
        display_max_len: 2000,
        output_json: false,
    };
    let (_helve, config) = build_helve_config(&opts);
    match build_react_run_context(&config).await {
        Ok(ctx) => match ctx.tool_source.list_tools().await {
            Ok(tools) => ServerResponse::ToolsList(ToolsListResponse { id, tools }),
            Err(e) => ServerResponse::Error(ErrorResponse {
                id: Some(id),
                error: e.to_string(),
            }),
        },
        Err(e) => ServerResponse::Error(ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}

async fn handle_tool_show(r: loom::ToolShowRequest) -> ServerResponse {
    let id = r.id.clone();
    let opts = RunOptions {
        message: String::new(),
        working_folder: None,
        thread_id: None,
        verbose: false,
        got_adaptive: false,
        display_max_len: 2000,
        output_json: false,
    };
    let (_helve, config) = build_helve_config(&opts);
    match build_react_run_context(&config).await {
        Ok(ctx) => match ctx.tool_source.list_tools().await {
            Ok(tools) => {
                let spec = tools.into_iter().find(|s| s.name == r.name);
                match spec {
                    Some(s) => {
                        let (tool, tool_yaml) = match r.output.as_ref() {
                            Some(ToolShowOutput::Yaml) => (
                                None,
                                Some(serde_yaml::to_string(&serde_json::json!({
                                    "name": s.name,
                                    "description": s.description,
                                    "input_schema": s.input_schema
                                })).unwrap_or_default()),
                            ),
                            _ => (
                                Some(serde_json::json!({
                                    "name": s.name,
                                    "description": s.description,
                                    "input_schema": s.input_schema
                                })),
                                None,
                            ),
                        };
                        ServerResponse::ToolShow(ToolShowResponse {
                            id,
                            tool,
                            tool_yaml,
                        })
                    }
                    None => ServerResponse::Error(ErrorResponse {
                        id: Some(id),
                        error: format!("tool not found: {}", r.name),
                    }),
                }
            }
            Err(e) => ServerResponse::Error(ErrorResponse {
                id: Some(id),
                error: e.to_string(),
            }),
        },
        Err(e) => ServerResponse::Error(ErrorResponse {
            id: Some(id),
            error: e.to_string(),
        }),
    }
}
