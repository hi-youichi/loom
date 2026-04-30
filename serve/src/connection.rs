use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use loom::cli_run::RunCancellation;
use loom::llm::ProviderConfig;
use loom::protocol::responses::CancelRunResponse;
use loom::{ClientRequest, ErrorResponse, ServerResponse};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

use super::agents::handle_agent_list;
use super::app::RunConfig;
use super::models::{handle_list_models, handle_set_model};
use super::run::handle_run;
use super::tools::{handle_tool_show, handle_tools_list};
use super::workspace::watcher::{WorkspaceWatcher, workspace_dir};
use loom::WorkspaceFileChangedResponse;

pub(crate) type SharedSink = Arc<Mutex<futures::stream::SplitSink<WebSocket, Message>>>;

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
    socket: WebSocket,
    shutdown_tx: Option<oneshot::Sender<()>>,
    workspace_store: Option<Arc<loom_workspace::Store>>,
    user_message_store: Option<std::sync::Arc<dyn loom::UserMessageStore>>,
    run_config: RunConfig,
    providers: Arc<Vec<ProviderConfig>>,
) {
    tracing::info!("🔗 New WebSocket connection established");

    let (sink, mut stream) = socket.split();
    let sink: SharedSink = Arc::new(Mutex::new(sink));

    let mut request_count = 0;
    let connection_start = std::time::Instant::now();
    let mut active_run_registry = ActiveRunRegistry::new();
    let (file_change_tx, mut file_change_rx) = tokio::sync::broadcast::channel::<WorkspaceFileChangedResponse>(64);
    let mut watchers: HashMap<String, WorkspaceWatcher> = HashMap::new();

    let notify_sink = sink.clone();
    let notify_handle = tokio::spawn(async move {
        loop {
            match file_change_rx.recv().await {
                Ok(notification) => {
                    tracing::info!(
                        "📤 Pushing file change notification: workspace={}, changes={}",
                        notification.workspace_id,
                        notification.changes.len()
                    );
                    let json = serde_json::to_string(&notification).unwrap_or_default();
                    let mut s = notify_sink.lock().await;
                    if s.send(Message::Text(json)).await.is_err() {
                        tracing::warn!("Failed to send file change notification");
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("File change notifications lagged: skipped {}", n);
                }
                Err(_) => break,
            }
        }
    });

    while let Some(res) = stream.next().await {
        let msg = match res {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("❌ WebSocket read error (client closed?): {}", e);
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
            &sink,
            workspace_store.clone(),
            user_message_store.clone(),
            &run_config,
            providers.clone(),
            &mut active_run_registry,
            &file_change_tx,
            &mut watchers,
        )
        .await
        {
            tracing::error!("❌ Request #{} failed: {}", request_count, e);
            break;
        }

        let duration = request_start.elapsed();
        tracing::debug!(
            "✅ Request #{} completed in {}ms",
            request_count,
            duration.as_millis()
        );
    }

    notify_handle.abort();

    let mut s = sink.lock().await;
    let _ = s.close().await;

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

#[allow(clippy::too_many_arguments)]
async fn handle_request_and_send(
    text: &str,
    sink: &SharedSink,
    workspace_store: Option<Arc<loom_workspace::Store>>,
    user_message_store: Option<std::sync::Arc<dyn loom::UserMessageStore>>,
    run_config: &RunConfig,
    providers: Arc<Vec<ProviderConfig>>,
    active_run_registry: &mut ActiveRunRegistry,
    file_change_tx: &tokio::sync::broadcast::Sender<WorkspaceFileChangedResponse>,
    watchers: &mut HashMap<String, WorkspaceWatcher>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let req: ClientRequest = match serde_json::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("⚠️  Failed to parse request: {}", e);
            let resp = ServerResponse::Error(ErrorResponse {
                id: None,
                error: format!("parse error: {}", e),
            });
            send_response_to_sink(sink, &resp).await?;
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
            match handle_run(r, sink, workspace_store, user_message_store, run_config).await {
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
            send_response_to_sink(
                sink,
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
            send_response_to_sink(sink, &resp).await?;
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
            send_response_to_sink(sink, &resp).await?;
            return Ok(());
        }
        ClientRequest::WorkspaceList(r) => {
            tracing::debug!("📂 Listing workspaces");
            super::workspace::handle_workspace_list(r, workspace_store.clone()).await
        }
        ClientRequest::WorkspaceCreate(r) => {
            tracing::debug!("📁 Creating workspace");
            let resp = super::workspace::handle_workspace_create(r, workspace_store.clone()).await;
            if let ServerResponse::WorkspaceCreate(ref create_resp) = resp {
                if let Some(dir) = workspace_dir(&create_resp.workspace_id) {
                    if !watchers.contains_key(&create_resp.workspace_id) {
                        let _ = std::fs::create_dir_all(&dir);
                        match WorkspaceWatcher::start(
                            create_resp.workspace_id.clone(),
                            dir,
                            file_change_tx.clone(),
                        ) {
                            Ok(w) => {
                                watchers.insert(create_resp.workspace_id.clone(), w);
                            }
                            Err(e) => {
                                tracing::warn!("Failed to start file watcher: {}", e);
                            }
                        }
                    }
                }
            }
            resp
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
        ClientRequest::WorkspaceFileList(r) => {
            tracing::debug!("📂 Listing workspace files: {}:{}", r.workspace_id, r.path.as_deref().unwrap_or(""));
            super::workspace::handle_workspace_file_list(r, workspace_store.clone()).await
        }
        ClientRequest::WorkspaceFileRead(r) => {
            tracing::debug!("📄 Reading workspace file: {}:{} ", r.workspace_id, r.path);
            super::workspace::handle_workspace_file_read(r, workspace_store.clone()).await
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
    send_response_to_sink(sink, &resp).await?;
    Ok(())
}

async fn send_response_to_sink(
    sink: &SharedSink,
    response: &ServerResponse,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let json = serde_json::to_string(response).unwrap_or_else(|_| {
        serde_json::to_string(&ServerResponse::Error(ErrorResponse {
            id: None,
            error: "serialization error".to_string(),
        }))
        .unwrap()
    });
    let mut s = sink.lock().await;
    s.send(Message::Text(json)).await?;
    Ok(())
}
