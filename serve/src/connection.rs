//! WebSocket connection lifecycle: recv loop and request dispatch.

use axum::extract::ws::{Message, WebSocket};
use loom::{ClientRequest, ErrorResponse, ServerResponse};
use std::sync::Arc;
use tokio::sync::oneshot;

use super::response::send_response;
use super::run::handle_run;
use super::tools::{handle_tool_show, handle_tools_list};

pub(crate) async fn handle_socket(
    mut socket: WebSocket,
    shutdown_tx: Option<oneshot::Sender<()>>,
    workspace_store: Option<Arc<loom_workspace::Store>>,
) {
    while let Some(res) = socket.recv().await {
        let msg = match res {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("read error (client closed?): {}", e);
                let _ = socket.close().await;
                break;
            }
        };
        let text = match &msg {
            Message::Text(t) => t.clone(),
            Message::Binary(b) => String::from_utf8_lossy(b).into_owned(),
            _ => continue,
        };

        if let Err(e) = handle_request_and_send(&text, &mut socket, workspace_store.clone()).await {
            tracing::warn!("handle_request error: {}", e);
            let _ = socket.close().await;
            break;
        }
    }
    if let Some(tx) = shutdown_tx {
        let _ = tx.send(());
    }
}

async fn handle_request_and_send(
    text: &str,
    socket: &mut WebSocket,
    workspace_store: Option<Arc<loom_workspace::Store>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let req: ClientRequest = match serde_json::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            let resp = ServerResponse::Error(ErrorResponse {
                id: None,
                error: format!("parse error: {}", e),
            });
            send_response(socket, &resp).await?;
            return Ok(());
        }
    };

    match req {
        ClientRequest::Run(r) => {
            if let Some(resp) = handle_run(r, socket, workspace_store).await? {
                send_response(socket, &resp).await?;
            }
        }
        ClientRequest::ToolsList(r) => {
            send_response(socket, &handle_tools_list(r).await).await?;
        }
        ClientRequest::ToolShow(r) => {
            send_response(socket, &handle_tool_show(r).await).await?;
        }
        ClientRequest::Ping(r) => {
            send_response(
                socket,
                &ServerResponse::Pong(loom::PongResponse { id: r.id }),
            )
            .await?;
        }
    }
    Ok(())
}
