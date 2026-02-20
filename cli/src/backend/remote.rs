//! RemoteBackend: run agent via WebSocket.

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use loom::{
    AgentType, ClientRequest, Envelope, RunCmd, RunError, RunOptions, RunRequest, ServerResponse,
    ToolShowOutput, ToolShowRequest, ToolsListRequest,
};
use super::RunOutput;
use crate::ToolShowFormat;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream,
};

use super::RunBackend;

const CONNECT_TIMEOUT_SECS: u64 = 10;
/// Max time to wait for each server message (run can take a long time for LLM).
const READ_TIMEOUT_SECS: u64 = 300;
pub struct RemoteBackend {
    url: String,
}

impl RemoteBackend {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }

    async fn connect(&self) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, RunError> {
        let (ws, _) = tokio::time::timeout(
            Duration::from_secs(CONNECT_TIMEOUT_SECS),
            connect_async(&self.url),
        )
        .await
        .map_err(|_| RunError::Remote("connect timeout".to_string()))?
        .map_err(|e| RunError::Remote(e.to_string()))?;
        Ok(ws)
    }

    fn cmd_to_agent(cmd: &RunCmd) -> AgentType {
        match cmd {
            RunCmd::React => AgentType::React,
            RunCmd::Dup => AgentType::Dup,
            RunCmd::Tot => AgentType::Tot,
            RunCmd::Got { .. } => AgentType::Got,
        }
    }

    fn run_request(id: &str, opts: &RunOptions, cmd: &RunCmd) -> ClientRequest {
        ClientRequest::Run(RunRequest {
            id: id.to_string(),
            message: opts.message.clone(),
            agent: Self::cmd_to_agent(cmd),
            thread_id: opts.thread_id.clone(),
            working_folder: opts
                .working_folder
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            got_adaptive: Some(opts.got_adaptive),
            verbose: Some(opts.verbose),
        })
    }
}

#[async_trait]
impl RunBackend for RemoteBackend {
    async fn run(
        &self,
        opts: &RunOptions,
        cmd: &RunCmd,
        stream_out: super::StreamOut,
    ) -> Result<RunOutput, RunError> {
        let ws = self.connect().await?;
        let (mut write, mut read) = ws.split();

        let id = format!(
            "req-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );
        let req = Self::run_request(&id, opts, cmd);
        let json = serde_json::to_string(&req).map_err(|e| RunError::Remote(e.to_string()))?;
        write
            .send(Message::Text(json))
            .await
            .map_err(|e| RunError::Remote(e.to_string()))?;

        let mut reply = None;
        let mut reply_envelope = None;
        let mut events: Vec<serde_json::Value> = Vec::new();
        let read_timeout = Duration::from_secs(READ_TIMEOUT_SECS);
        loop {
            let next = tokio::time::timeout(read_timeout, read.next()).await;
            let res = match next {
                Ok(Some(r)) => r,
                Ok(None) => break,
                Err(_) => return Err(RunError::Remote("read timeout (no response from server)".to_string())),
            };
            let msg = res.map_err(|e| RunError::Remote(e.to_string()))?;
            if !msg.is_text() {
                continue;
            }
            let text = msg.to_text().unwrap_or("");
            let resp: ServerResponse =
                serde_json::from_str(text).map_err(|e| RunError::Remote(e.to_string()))?;
            match resp {
                ServerResponse::RunStreamEvent(r) if r.id == id => {
                    if let Some(ref out) = stream_out {
                        if let Ok(mut f) = out.lock() {
                            f(r.event);
                        }
                    } else {
                        events.push(r.event);
                    }
                }
                ServerResponse::RunEnd(r) if r.id == id => {
                    reply = Some(r.reply);
                    let (s, n, e) = (r.session_id, r.node_id, r.event_id);
                    reply_envelope = (s.is_some() || n.is_some() || e.is_some()).then(|| {
                        Envelope::new()
                            .with_session_id(s.unwrap_or_default())
                            .with_node_id(n.unwrap_or_default())
                            .with_event_id(e.unwrap_or(0))
                    });
                    break;
                }
                ServerResponse::Error(e) if e.id.as_deref() == Some(&id) => {
                    return Err(RunError::Remote(e.error));
                }
                ServerResponse::Error(e) => return Err(RunError::Remote(e.error)),
                _ => {}
            }
        }
        let reply = reply.ok_or_else(|| RunError::Remote("no run_end received".to_string()))?;
        Ok(if stream_out.is_some() {
            RunOutput::Reply(reply, reply_envelope)
        } else if opts.output_json {
            RunOutput::Json {
                events,
                reply,
                reply_envelope,
            }
        } else {
            RunOutput::Reply(reply, reply_envelope)
        })
    }

    async fn list_tools(&self, opts: &RunOptions) -> Result<(), RunError> {
        let ws = self.connect().await?;
        let (mut write, mut read) = ws.split();

        let id = format!(
            "req-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );
        let req = ClientRequest::ToolsList(ToolsListRequest {
            id: id.clone(),
            working_folder: opts
                .working_folder
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            thread_id: opts.thread_id.clone(),
        });
        let json = serde_json::to_string(&req).map_err(|e| RunError::Remote(e.to_string()))?;
        write
            .send(Message::Text(json))
            .await
            .map_err(|e| RunError::Remote(e.to_string()))?;

        while let Some(res) = read.next().await {
            let msg = res.map_err(|e| RunError::Remote(e.to_string()))?;
            if !msg.is_text() {
                continue;
            }
            let text = msg.to_text().unwrap_or("");
            let resp: ServerResponse =
                serde_json::from_str(text).map_err(|e| RunError::Remote(e.to_string()))?;
            match resp {
                ServerResponse::ToolsList(r) if r.id == id => {
                    return crate::tool_cmd::format_tools_list(&r.tools, opts.output_json);
                }
                ServerResponse::Error(e) if e.id.as_deref() == Some(&id) => {
                    return Err(RunError::Remote(e.error));
                }
                ServerResponse::Error(e) => return Err(RunError::Remote(e.error)),
                _ => {}
            }
        }
        Err(RunError::Remote("no tools_list received".to_string()))
    }

    async fn show_tool(
        &self,
        opts: &RunOptions,
        name: &str,
        format: ToolShowFormat,
    ) -> Result<(), RunError> {
        let ws = self.connect().await?;
        let (mut write, mut read) = ws.split();

        let id = format!(
            "req-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );
        let output = match format {
            ToolShowFormat::Yaml => Some(ToolShowOutput::Yaml),
            ToolShowFormat::Json => Some(ToolShowOutput::Json),
        };
        let req = ClientRequest::ToolShow(ToolShowRequest {
            id: id.clone(),
            name: name.to_string(),
            output,
            working_folder: opts
                .working_folder
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            thread_id: opts.thread_id.clone(),
        });
        let json = serde_json::to_string(&req).map_err(|e| RunError::Remote(e.to_string()))?;
        write
            .send(Message::Text(json))
            .await
            .map_err(|e| RunError::Remote(e.to_string()))?;

        while let Some(res) = read.next().await {
            let msg = res.map_err(|e| RunError::Remote(e.to_string()))?;
            if !msg.is_text() {
                continue;
            }
            let text = msg.to_text().unwrap_or("");
            let resp: ServerResponse =
                serde_json::from_str(text).map_err(|e| RunError::Remote(e.to_string()))?;
            match resp {
                ServerResponse::ToolShow(r) if r.id == id => {
                    return crate::tool_cmd::format_tool_show_output(&r, format);
                }
                ServerResponse::Error(e) if e.id.as_deref() == Some(&id) => {
                    return Err(RunError::Remote(e.error));
                }
                ServerResponse::Error(e) => return Err(RunError::Remote(e.error)),
                _ => {}
            }
        }
        Err(RunError::Remote("no tool_show received".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use loom::{ErrorResponse, RunEndResponse, RunStreamEventResponse, ToolShowResponse, ToolSpec, ToolsListResponse};
    use serde_json::Value;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    #[test]
    fn cmd_to_agent_maps_all_variants() {
        assert!(matches!(
            RemoteBackend::cmd_to_agent(&RunCmd::React),
            AgentType::React
        ));
        assert!(matches!(
            RemoteBackend::cmd_to_agent(&RunCmd::Dup),
            AgentType::Dup
        ));
        assert!(matches!(
            RemoteBackend::cmd_to_agent(&RunCmd::Tot),
            AgentType::Tot
        ));
        assert!(matches!(
            RemoteBackend::cmd_to_agent(&RunCmd::Got { got_adaptive: true }),
            AgentType::Got
        ));
    }

    #[test]
    fn run_request_maps_options_to_payload() {
        let opts = RunOptions {
            message: "hello".to_string(),
            working_folder: Some(PathBuf::from("/tmp/project")),
            thread_id: Some("thread-1".to_string()),
            verbose: true,
            got_adaptive: true,
            display_max_len: 120,
            output_json: true,
        };
        let req = RemoteBackend::run_request("req-1", &opts, &RunCmd::Tot);
        match req {
            ClientRequest::Run(r) => {
                assert_eq!(r.id, "req-1");
                assert_eq!(r.message, "hello");
                assert!(matches!(r.agent, AgentType::Tot));
                assert_eq!(r.thread_id.as_deref(), Some("thread-1"));
                assert_eq!(r.working_folder.as_deref(), Some("/tmp/project"));
                assert_eq!(r.got_adaptive, Some(true));
                assert_eq!(r.verbose, Some(true));
            }
            _ => panic!("expected ClientRequest::Run"),
        }
    }

    #[test]
    fn new_stores_url() {
        let backend = RemoteBackend::new("ws://localhost:8080");
        assert_eq!(backend.url, "ws://localhost:8080");
    }

    fn opts(output_json: bool) -> RunOptions {
        RunOptions {
            message: "hello".to_string(),
            working_folder: None,
            thread_id: Some("thread-1".to_string()),
            verbose: false,
            got_adaptive: false,
            display_max_len: 120,
            output_json,
        }
    }

    #[tokio::test]
    async fn run_returns_json_events_and_reply_without_stream_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            let req_msg = ws.next().await.unwrap().unwrap();
            let req_text = req_msg.to_text().unwrap();
            let req: ClientRequest = serde_json::from_str(req_text).unwrap();
            let id = match req {
                ClientRequest::Run(r) => r.id,
                _ => panic!("expected run request"),
            };
            let ev = ServerResponse::RunStreamEvent(RunStreamEventResponse {
                id: id.clone(),
                event: serde_json::json!({"type":"node_enter","id":"think"}),
            });
            ws.send(Message::Text(serde_json::to_string(&ev).unwrap().into()))
                .await
                .unwrap();
            let end = ServerResponse::RunEnd(RunEndResponse {
                id,
                reply: "done".to_string(),
                usage: None,
                total_usage: None,
                session_id: Some("sess-1".to_string()),
                node_id: Some("think".to_string()),
                event_id: Some(3),
            });
            ws.send(Message::Text(serde_json::to_string(&end).unwrap().into()))
                .await
                .unwrap();
        });

        let backend = RemoteBackend::new(format!("ws://{}", addr));
        let out = backend
            .run(&opts(true), &RunCmd::React, None)
            .await
            .unwrap();
        match out {
            RunOutput::Json {
                events,
                reply,
                reply_envelope,
            } => {
                assert_eq!(reply, "done");
                assert_eq!(events.len(), 1);
                let env = reply_envelope.unwrap();
                assert_eq!(env.session_id.as_deref(), Some("sess-1"));
            }
            _ => panic!("expected json output"),
        }
    }

    #[tokio::test]
    async fn run_with_stream_out_forwards_events_and_returns_reply() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            let req_msg = ws.next().await.unwrap().unwrap();
            let req_text = req_msg.to_text().unwrap();
            let req: ClientRequest = serde_json::from_str(req_text).unwrap();
            let id = match req {
                ClientRequest::Run(r) => r.id,
                _ => panic!("expected run request"),
            };
            let ev = ServerResponse::RunStreamEvent(RunStreamEventResponse {
                id: id.clone(),
                event: serde_json::json!({"type":"usage","total_tokens":7}),
            });
            ws.send(Message::Text(serde_json::to_string(&ev).unwrap().into()))
                .await
                .unwrap();
            let end = ServerResponse::RunEnd(RunEndResponse {
                id,
                reply: "ok".to_string(),
                usage: None,
                total_usage: None,
                session_id: None,
                node_id: None,
                event_id: None,
            });
            ws.send(Message::Text(serde_json::to_string(&end).unwrap().into()))
                .await
                .unwrap();
        });

        let captured: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);
        let sink: super::super::StreamOut = Some(Arc::new(Mutex::new(move |v: Value| {
            captured_clone.lock().unwrap().push(v);
        })));

        let backend = RemoteBackend::new(format!("ws://{}", addr));
        let out = backend.run(&opts(true), &RunCmd::React, sink).await.unwrap();
        assert!(matches!(out, RunOutput::Reply(reply, _) if reply == "ok"));
        assert_eq!(captured.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn run_returns_error_for_matching_error_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            let req_msg = ws.next().await.unwrap().unwrap();
            let req_text = req_msg.to_text().unwrap();
            let req: ClientRequest = serde_json::from_str(req_text).unwrap();
            let id = match req {
                ClientRequest::Run(r) => r.id,
                _ => panic!("expected run request"),
            };
            let err = ServerResponse::Error(ErrorResponse {
                id: Some(id),
                error: "remote boom".to_string(),
            });
            ws.send(Message::Text(serde_json::to_string(&err).unwrap().into()))
                .await
                .unwrap();
        });

        let backend = RemoteBackend::new(format!("ws://{}", addr));
        let err = backend
            .run(&opts(false), &RunCmd::React, None)
            .await
            .unwrap_err();
        assert!(matches!(err, RunError::Remote(msg) if msg == "remote boom"));
    }

    #[tokio::test]
    async fn list_tools_and_show_tool_handle_success_responses() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            // First connection: tools/list
            let (stream1, _) = listener.accept().await.unwrap();
            let mut ws1 = accept_async(stream1).await.unwrap();
            let req1_msg = ws1.next().await.unwrap().unwrap();
            let req1: ClientRequest = serde_json::from_str(req1_msg.to_text().unwrap()).unwrap();
            let id1 = match req1 {
                ClientRequest::ToolsList(r) => r.id,
                _ => panic!("expected tools/list"),
            };
            let list = ServerResponse::ToolsList(ToolsListResponse {
                id: id1,
                tools: vec![ToolSpec {
                    name: "demo".to_string(),
                    description: Some("d".to_string()),
                    input_schema: serde_json::json!({"type":"object"}),
                }],
            });
            ws1.send(Message::Text(serde_json::to_string(&list).unwrap().into()))
                .await
                .unwrap();

            // Second connection: tool/show
            let (stream2, _) = listener.accept().await.unwrap();
            let mut ws2 = accept_async(stream2).await.unwrap();
            let req2_msg = ws2.next().await.unwrap().unwrap();
            let req2: ClientRequest = serde_json::from_str(req2_msg.to_text().unwrap()).unwrap();
            let id2 = match req2 {
                ClientRequest::ToolShow(r) => r.id,
                _ => panic!("expected tool/show"),
            };
            let show = ServerResponse::ToolShow(ToolShowResponse {
                id: id2,
                tool: Some(
                    serde_json::to_value(ToolSpec {
                        name: "demo".to_string(),
                        description: Some("d".to_string()),
                        input_schema: serde_json::json!({"type":"object"}),
                    })
                    .unwrap(),
                ),
                tool_yaml: None,
            });
            ws2.send(Message::Text(serde_json::to_string(&show).unwrap().into()))
                .await
                .unwrap();
        });

        let backend = RemoteBackend::new(format!("ws://{}", addr));
        backend.list_tools(&opts(true)).await.unwrap();
        backend
            .show_tool(&opts(true), "demo", ToolShowFormat::Json)
            .await
            .unwrap();
    }
}
