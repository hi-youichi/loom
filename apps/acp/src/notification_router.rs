//! Route session notifications to the transport currently bound to a session.

use std::sync::Arc;

use agent_client_protocol::schema::v1::SessionNotification;
use tokio::sync::oneshot;

use crate::connection::{ConnectionOutbound, ConnectionStateError};
use crate::connection_registry::ConnectionRegistry;
use crate::session::SessionId;
use crate::session_bindings::SessionBindings;

#[derive(Debug, thiserror::Error)]
pub enum NotificationRouteError {
    #[error("session is not bound to a connection: {0}")]
    Unbound(SessionId),
    #[error("bound connection is not active: {0}")]
    ConnectionUnavailable(String),
    #[error("connection outbound queue is closed")]
    QueueClosed,
    #[error("notification flush acknowledgement was dropped")]
    FlushDropped,
    #[error(transparent)]
    ConnectionState(#[from] ConnectionStateError),
}

#[derive(Debug)]
pub struct NotificationRouter {
    bindings: Arc<SessionBindings>,
    connections: Arc<ConnectionRegistry>,
}

impl NotificationRouter {
    pub fn new(bindings: Arc<SessionBindings>, connections: Arc<ConnectionRegistry>) -> Self {
        Self {
            bindings,
            connections,
        }
    }

    pub async fn send(
        &self,
        notification: SessionNotification,
    ) -> Result<(), NotificationRouteError> {
        self.route(notification, None).await
    }

    /// Route a batch and wait until the final notification has been accepted
    /// by the SDK connection. Callers may then enqueue the JSON-RPC response.
    pub async fn send_and_flush(
        &self,
        notifications: impl IntoIterator<Item = SessionNotification>,
    ) -> Result<(), NotificationRouteError> {
        let mut values = notifications.into_iter().peekable();
        let mut final_ack = None;
        while let Some(notification) = values.next() {
            if values.peek().is_some() {
                self.route(notification, None).await?;
            } else {
                let (ack_tx, ack_rx) = oneshot::channel();
                self.route(notification, Some(ack_tx)).await?;
                final_ack = Some(ack_rx);
            }
        }
        if let Some(ack) = final_ack {
            ack.await
                .map_err(|_| NotificationRouteError::FlushDropped)?;
        }
        Ok(())
    }

    /// Wait until all outbound items previously routed for this session have
    /// passed through the connection drain.
    pub async fn flush_session(
        &self,
        session_id: &SessionId,
    ) -> Result<(), NotificationRouteError> {
        let connection_id = self
            .bindings
            .connection_for(session_id)
            .ok_or_else(|| NotificationRouteError::Unbound(session_id.clone()))?;
        let connection = self
            .connections
            .get(&connection_id)
            .filter(|connection| connection.is_active())
            .ok_or(NotificationRouteError::ConnectionUnavailable(connection_id))?;
        let (ack_tx, ack_rx) = oneshot::channel();
        connection
            .outbound_tx
            .send(ConnectionOutbound::Barrier(ack_tx))
            .await
            .map_err(|_| NotificationRouteError::QueueClosed)?;
        ack_rx
            .await
            .map_err(|_| NotificationRouteError::FlushDropped)
    }

    async fn route(
        &self,
        notification: SessionNotification,
        enqueued: Option<oneshot::Sender<()>>,
    ) -> Result<(), NotificationRouteError> {
        let session_id = SessionId::new(notification.session_id.to_string());
        let connection_id = self
            .bindings
            .connection_for(&session_id)
            .ok_or_else(|| NotificationRouteError::Unbound(session_id.clone()))?;
        let connection = self
            .connections
            .get(&connection_id)
            .filter(|connection| connection.is_active())
            .ok_or_else(|| NotificationRouteError::ConnectionUnavailable(connection_id.clone()))?;
        if !connection.is_initialized() {
            return Err(ConnectionStateError::NotInitialized.into());
        }
        connection
            .outbound_tx
            .send(ConnectionOutbound::Notification {
                value: notification,
                enqueued,
            })
            .await
            .map_err(|_| NotificationRouteError::QueueClosed)
    }
}
