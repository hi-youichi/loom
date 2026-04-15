//! WebSocket connection lifecycle: recv loop and request dispatch.

use axum::extract::ws::{Message, WebSocket};
use loom::cli_run::RunCancellation;
use loom::llm::ProviderConfig;
use loom::protocol::responses::CancelRunResponse;
use loom::{ClientRequest, ErrorResponse, ServerResponse};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::oneshot;

use super::agents::handle_agent_list;
use super::app::RunConfig;
use super::models::{handle_list_models, handle_set_model};
use super::response::send_response;
use super::run::handle_run;
use super::tools::{handle_tool_show, handle_tools_list};

/// Registry for tracking active runs and their cancellation handles.
struct ActiveRunRegistry {
    runs: HashMap<String, RunCancellation>,
}

impl ActiveRunRegistry {
    fn new() -> Self {
        Self {
            runs: HashMap::new(),
        }
    }

    fn insert(&mut self, run_id: String, cancellation: RunCancellation) {
        self.runs.insert(run_id, cancellation);
    }

    fn cancel(&mut self, run_id: &str) -> bool {
        if let Some(cancellation) = self.runs.remove(run_id) {
            cancellation.cancel();
            true
        } else {
            false
        }
    }
}

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
    let mut active_run_registry = ActiveRunRegistry::new();

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
        tracing::debug!(
            "📨 Request #{}: {}",
            request_count,
            text.chars().take(100).collect::<String>()
        );

        let request_start = std::time::Instant::now();

        if let Err(e) = handle_request_and_send(
            &text,
            &mut socket,
            workspace_store.clone(),
            user_message_store.clone(),
            &run_config,
            providers.clone(),
            &mut active_run_registry,
        )
        .await
        {
            tracing::error!("❌ Request #{} failed: {}", request_count, e);
            let _ = socket.close().await;
            break;
        }

        let duration = request_start.elapsed();
        tracing::debug!(
            "✅ Request #{} completed in {}ms",
            request_count,
            duration.as_millis()
        );
    }

    let connection_duration = connection_start.elapsed();
    tracing::info!(
        "🔌 WebSocket connection closed (handled {} requests in {}ms)",
        request_count,
        connection_duration.as_millis()
    );

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
    active_run_registry: &mut ActiveRunRegistry,
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
    tracing::info!(
        "Handling request: {} (id: {:?})",
        request_type,
        match &req {
            ClientRequest::Run(r) => r.id.clone(),
            ClientRequest::ListModels(r) => Some(r.id.clone()),
            ClientRequest::SetModel(r) => Some(r.id.clone()),
            ClientRequest::CancelRun(r) => Some(r.id.clone()),
            _ => None,
        }
    );

    let resp = match req {
        ClientRequest::Run(r) => {
            tracing::info!("Starting agent run with profile: {}", r.agent);
            let request_id = r.id.clone();
            match handle_run(r, socket, workspace_store, user_message_store, run_config).await {
                Ok((run_id, cancellation, Some(resp))) => {
                    active_run_registry.insert(run_id, cancellation);
                    tracing::info!("Run completed with response");
                    resp
                }
                Ok((run_id, cancellation, None)) => {
                    active_run_registry.insert(run_id, cancellation);
                    tracing::info!("Run streamed to client");
                    return Ok(());
                }
                Err(e) => {
                    tracing::error!("Run failed: {}", e);
                    ServerResponse::Error(ErrorResponse {
                        id: request_id,
                        error: e.to_string(),
                    })
                }
            }
        }
        ClientRequest::ToolsList(r) => {
            tracing::debug!("🔧 Listing available tools");
            handle_tools_list(r, run_config).await
        }
        ClientRequest::ToolShow(r) => {
            tracing::debug!("🔧 Showing tool details: {}", r.name);
            handle_tool_show(r, run_config).await
        }
        ClientRequest::AgentList(r) => {
            tracing::debug!("📋 Listing available agents");
            handle_agent_list(r).await
        }
        ClientRequest::UserMessages(r) => {
            tracing::debug!("💬 Handling user messages for thread: {}", r.thread_id);
            super::user_messages::handle_user_messages(r, user_message_store).await
        }
        ClientRequest::Ping(r) => {
            tracing::debug!("🏓 Ping received");
            send_response(
                socket,
                &ServerResponse::Pong(loom::PongResponse { id: r.id }),
            )
            .await?;
            return Ok(());
        }
        ClientRequest::ListModels(r) => {
            tracing::debug!("📋 Listing available models");
            let resp = handle_list_models(r, &providers).await;
            match &resp {
                ServerResponse::ListModels(m) => {
                    tracing::info!("📋 Listed {} models", m.models.len());
                }
                ServerResponse::Error(e) => {
                    tracing::error!("❌ Failed to list models: {}", e.error);
                }
                _ => {}
            }
            send_response(socket, &resp).await?;
            return Ok(());
        }
        ClientRequest::SetModel(r) => {
            tracing::info!(
                "🔄 Setting model: {} for session: {}",
                r.model_id,
                r.session_id.as_deref().unwrap_or("default")
            );
            let resp = handle_set_model(r, &providers).await;
            match &resp {
                ServerResponse::SetModel(_) => tracing::info!("✅ Model set successfully"),
                ServerResponse::Error(e) => tracing::error!("❌ Failed to set model: {}", e.error),
                _ => {}
            }
            send_response(socket, &resp).await?;
            return Ok(());
        }
        ClientRequest::WorkspaceList(r) => {
            tracing::debug!("📂 Listing workspaces");
            super::workspace::handle_workspace_list(r, workspace_store.clone()).await
        }
        ClientRequest::WorkspaceCreate(r) => {
            tracing::debug!("📁 Creating workspace");
            super::workspace::handle_workspace_create(r, workspace_store.clone()).await
        }
        ClientRequest::WorkspaceThreadList(r) => {
            tracing::debug!("📋 Listing workspace threads");
            super::workspace::handle_workspace_thread_list(r, workspace_store.clone()).await
        }
        ClientRequest::WorkspaceThreadAdd(r) => {
            tracing::debug!("➕ Adding thread to workspace");
            super::workspace::handle_workspace_thread_add(r, workspace_store.clone()).await
        }
        ClientRequest::WorkspaceThreadRemove(r) => {
            tracing::debug!("➖ Removing thread from workspace");
            super::workspace::handle_workspace_thread_remove(r, workspace_store.clone()).await
        }
        ClientRequest::WorkspaceRename(r) => {
            tracing::debug!("✏️ Renaming workspace");
            super::workspace::handle_workspace_rename(r, workspace_store.clone()).await
        }
        ClientRequest::CancelRun(r) => {
            tracing::info!("🛑 Cancelling run: {}", r.run_id);
            if active_run_registry.cancel(&r.run_id) {
                ServerResponse::CancelRun(CancelRunResponse {
                    id: r.id,
                    run_id: r.run_id,
                })
            } else {
                ServerResponse::Error(ErrorResponse {
                    id: Some(r.id),
                    error: format!("Run {} not found or already completed", r.run_id),
                })
            }
        }
    };

    tracing::debug!("📤 Sending response for: {}", request_type);
    send_response(socket, &resp).await?;
    Ok(())
}
