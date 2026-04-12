//! WebSocket connection lifecycle: recv loop and request dispatch.

use axum::extract::ws::{Message, WebSocket};
use loom::{ClientRequest, ErrorResponse, ServerResponse};
use loom::llm::ProviderConfig;
use std::sync::Arc;
use tokio::sync::oneshot;

use super::app::RunConfig;
use super::response::send_response;
use super::run::handle_run;
use super::tools::{handle_tool_show, handle_tools_list};
use super::user_messages::handle_user_messages;
use super::agents::handle_agent_list;
use super::models::{handle_list_models, handle_set_model};
use super::workspace::{
    handle_workspace_create, handle_workspace_list, handle_workspace_thread_add,
    handle_workspace_thread_list, handle_workspace_thread_remove,
};

pub(crate) async fn handle_socket(
    mut socket: WebSocket,
    shutdown_tx: Option<oneshot::Sender<()>>,
    workspace_store: Option<Arc<loom_workspace::Store>>,
    user_message_store: Option<std::sync::Arc<dyn loom::UserMessageStore>>,
    run_config: RunConfig,
    providers: Arc<Vec<ProviderConfig>>,
) {
    tracing::info!("🔗 New WebSocket connection established");
    
    let mut request_count = 0;
    let connection_start = std::time::Instant::now();

    while let Some(res) = socket.recv().await {
        let msg = match res {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("❌ WebSocket read error (client closed?): {}", e);
                let _ = socket.close().await;
                break;
            }
        };
        let text = match &msg {
            Message::Text(t) => t.clone(),
            Message::Binary(b) => String::from_utf8_lossy(b).into_owned(),
            _ => {
                tracing::debug!("Received non-text message, skipping");
                continue;
            }
        };

        request_count += 1;
        tracing::debug!("📨 Request #{}: {}", request_count, text.chars().take(100).collect::<String>());

        let request_start = std::time::Instant::now();
        
        if let Err(e) = handle_request_and_send(
            &text,
            &mut socket,
            workspace_store.clone(),
            user_message_store.clone(),
            &run_config,
            providers.clone(),
        )
        .await
        {
            tracing::error!("❌ Request #{} failed: {}", request_count, e);
            let _ = socket.close().await;
            break;
        }
        
        let duration = request_start.elapsed();
        tracing::debug!("✅ Request #{} completed in {}ms", request_count, duration.as_millis());
    }
    
    let connection_duration = connection_start.elapsed();
    tracing::info!("🔌 WebSocket connection closed (handled {} requests in {}ms)", 
        request_count, connection_duration.as_millis());
    
    if let Some(tx) = shutdown_tx {
        let _ = tx.send(());
    }
}

async fn handle_request_and_send(
    text: &str,
    socket: &mut WebSocket,
    workspace_store: Option<Arc<loom_workspace::Store>>,
    user_message_store: Option<std::sync::Arc<dyn loom::UserMessageStore>>,
    run_config: &RunConfig,
    providers: Arc<Vec<ProviderConfig>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let req: ClientRequest = match serde_json::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("⚠️  Failed to parse request: {}", e);
            let resp = ServerResponse::Error(ErrorResponse {
                id: None,
                error: format!("parse error: {}", e),
            });
            send_response(socket, &resp).await?;
            return Ok(());
        }
    };

    let request_type = format!("{:?}", req);
    tracing::info!("🎯 Handling request: {} (id: {:?})", request_type, 
        match &req {
            ClientRequest::Run(r) => Some(r.id.clone()),
            ClientRequest::ToolsList(r) => Some(r.id.clone()),
            ClientRequest::ToolShow(r) => Some(r.id.clone()),
            ClientRequest::AgentList(r) => Some(r.id.clone()),
            ClientRequest::Ping(r) => Some(r.id.clone()),
            ClientRequest::ListModels(r) => Some(r.id.clone()),
            ClientRequest::SetModel(r) => Some(r.id.clone()),
            _ => None,
        }
    );

    let resp = match req {
        ClientRequest::Run(r) => {
            tracing::info!("🚀 Starting agent run with profile: {}", r.agent);
            let resp = handle_run(r, workspace_store, user_message_store, run_config).await;
            match &resp {
                ServerResponse::RunStart(_) => tracing::info!("✅ Run started successfully"),
                ServerResponse::Error(e) => tracing::error!("❌ Run failed: {}", e.error),
                _ => {}
            }
            resp
        }
        ClientRequest::ToolsList(r) => {
            tracing::debug!("📋 Listing available tools");
            handle_tools_list(r).await
        }
        ClientRequest::ToolShow(r) => {
            tracing::debug!("🔍 Showing tool details: {}", r.name);
            handle_tool_show(r).await
        }
        ClientRequest::AgentList(r) => {
            tracing::debug!("👥 Listing available agents");
            handle_agent_list(r).await
        }
        ClientRequest::Ping(r) => {
            tracing::debug!("💓 Ping received");
            send_response(
                socket,
                &ServerResponse::Pong(loom::PongResponse { id: r.id }),
            )
            .await?;
            return Ok(());
        }
        ClientRequest::ListModels(r) => {
            tracing::debug!("🤖 Listing available models");
            let resp = handle_list_models(r, &providers).await;
            match &resp {
                ServerResponse::ModelsList(m) => {
                    tracing::info!("✅ Listed {} models", m.models.len());
                }
                ServerResponse::Error(e) => {
                    tracing::error!("❌ Failed to list models: {}", e.error);
                }
                _ => {}
            }
            resp
        }
        ClientRequest::SetModel(r) => {
            tracing::info!("⚙️  Setting model: {} for session: {}", r.model_id, 
                r.session_id.as_deref().unwrap_or("default"));
            let resp = handle_set_model(r, &providers).await;
            match &resp {
                ServerResponse::SetModel(_) => tracing::info!("✅ Model set successfully"),
                ServerResponse::Error(e) => tracing::error!("❌ Failed to set model: {}", e.error),
                _ => {}
            }
            resp
        }
    };
    
    tracing::debug!("📤 Sending response for: {}", request_type);
    send_response(socket, &resp).await?;
    Ok(())
}
    };

    match req {
        ClientRequest::Run(r) => {
            if let Some(resp) =
                handle_run(r, socket, workspace_store, user_message_store, run_config).await?
            {
                send_response(socket, &resp).await?;
            }
        }
        ClientRequest::ToolsList(r) => {
            send_response(socket, &handle_tools_list(r, run_config).await).await?;
        }
        ClientRequest::ToolShow(r) => {
            send_response(socket, &handle_tool_show(r, run_config).await).await?;
        }
        ClientRequest::UserMessages(r) => {
            let resp = handle_user_messages(r, user_message_store.clone()).await;
            send_response(socket, &resp).await?;
        }
        ClientRequest::AgentList(r) => {
            let resp = handle_agent_list(r).await;
            send_response(socket, &resp).await?;
        }
        ClientRequest::WorkspaceList(r) => {
            let resp = handle_workspace_list(r, workspace_store.clone()).await;
            send_response(socket, &resp).await?;
        }
        ClientRequest::WorkspaceCreate(r) => {
            let resp = handle_workspace_create(r, workspace_store.clone()).await;
            send_response(socket, &resp).await?;
        }
        ClientRequest::WorkspaceThreadList(r) => {
            let resp = handle_workspace_thread_list(r, workspace_store.clone()).await;
            send_response(socket, &resp).await?;
        }
        ClientRequest::WorkspaceThreadAdd(r) => {
            let resp = handle_workspace_thread_add(r, workspace_store.clone()).await;
            send_response(socket, &resp).await?;
        }
        ClientRequest::WorkspaceThreadRemove(r) => {
            let resp = handle_workspace_thread_remove(r, workspace_store.clone()).await;
            send_response(socket, &resp).await?;
        }
        ClientRequest::Ping(r) => {
            send_response(
                socket,
                &ServerResponse::Pong(loom::PongResponse { id: r.id }),
            )
            .await?;
        }
        ClientRequest::ListModels(r) => {
            let resp = handle_list_models(r, &providers).await;
            send_response(socket, &resp).await?;
        }
        ClientRequest::SetModel(r) => {
            let resp = handle_set_model(r, &providers).await;
            send_response(socket, &resp).await?;
        }
    }
    Ok(())
}
