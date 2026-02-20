//! Send a single `ServerResponse` as JSON over the WebSocket.

use axum::extract::ws::{Message, WebSocket};
use loom::{ErrorResponse, ServerResponse};

pub(crate) async fn send_response(
    socket: &mut WebSocket,
    response: &ServerResponse,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let json = serde_json::to_string(response).unwrap_or_else(|_| {
        serde_json::to_string(&ServerResponse::Error(ErrorResponse {
            id: None,
            error: "serialization error".to_string(),
        }))
        .unwrap()
    });
    socket.send(Message::Text(json)).await?;
    Ok(())
}
