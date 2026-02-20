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
        let req = ClientRequest::ToolsList(ToolsListRequest { id: id.clone() });
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
        _opts: &RunOptions,
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
