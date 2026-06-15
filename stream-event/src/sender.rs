use crate::{MessageChunk, MessageChunkKind, StreamEvent, StreamMetadata};
use std::fmt::Debug;
use tokio::sync::mpsc;

/// Adapter that converts `MessageChunk` into `StreamEvent::Messages` and sends to `stream_tx`.
///
/// Used by ThinkNode (and similar nodes) to avoid manual channel setup and forward loops.
/// Call `channel()` to get (chunk_tx, chunk_rx), pass `chunk_tx` to `invoke_stream`, then
/// `forward(chunk_rx)` alongside it with `tokio::join!` so all chunks are forwarded before return.
pub struct ChunkToStreamSender<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    stream_tx: mpsc::Sender<StreamEvent<S>>,
    node_id: String,
    namespace: Option<String>,
}

impl<S> ChunkToStreamSender<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    pub fn new(stream_tx: mpsc::Sender<StreamEvent<S>>, node_id: impl Into<String>) -> Self {
        Self {
            stream_tx,
            node_id: node_id.into(),
            namespace: None,
        }
    }

    pub fn new_with_namespace(
        stream_tx: mpsc::Sender<StreamEvent<S>>,
        node_id: impl Into<String>,
        namespace: Option<String>,
    ) -> Self {
        Self {
            stream_tx,
            node_id: node_id.into(),
            namespace,
        }
    }

    /// Returns (chunk_tx, chunk_rx). Pass chunk_tx to `invoke_stream`, then await
    /// `forward(chunk_rx)` together with invoke_stream via `tokio::join!` so forwarding
    /// completes before the caller returns.
    pub fn channel(&self) -> (mpsc::Sender<MessageChunk>, mpsc::Receiver<MessageChunk>) {
        mpsc::channel::<MessageChunk>(128)
    }

    /// Forwards chunks from `chunk_rx` to `stream_tx` as `StreamEvent::Messages`.
    /// Completes when `chunk_rx` is closed (e.g. when invoke_stream drops its sender).
    ///
    /// Returns `(count, first_token_at)` where `first_token_at` is the `Instant` at which
    /// the very first chunk was received (used by callers to compute prefill/decode durations).
    pub async fn forward(
        &self,
        mut chunk_rx: mpsc::Receiver<MessageChunk>,
    ) -> (usize, Option<std::time::Instant>) {
        let stream_tx = self.stream_tx.clone();
        let node_id = self.node_id.clone();
        let namespace = self.namespace.clone();
        let mut forwarded = 0usize;
        let mut first_token_at: Option<std::time::Instant> = None;
        tracing::info!(
            hang_probe = "ChunkToStreamSender::forward",
            "hang_probe: forward enter"
        );
        while let Some(chunk) = chunk_rx.recv().await {
            let kind_label = match &chunk.kind {
                MessageChunkKind::Message => "message",
                MessageChunkKind::Thinking => "thinking",
            };
            if first_token_at.is_none() {
                first_token_at = Some(std::time::Instant::now());
                tracing::info!(
                    hang_probe = "ChunkToStreamSender::forward",
                    kind = kind_label,
                    "hang_probe: forward first chunk"
                );
            }
            forwarded += 1;
            if forwarded.is_multiple_of(50) {
                tracing::info!(
                    hang_probe = "ChunkToStreamSender::forward",
                    forwarded,
                    "hang_probe: forward progress"
                );
            }
            let event = StreamEvent::Messages {
                chunk,
                metadata: StreamMetadata {
                    loom_node: node_id.clone(),
                    namespace: namespace.clone(),
                },
            };
            let send_start = std::time::Instant::now();
            tracing::trace!(
                hang_probe = "ChunkToStreamSender::forward",
                forwarded,
                "hang_probe: forward send start"
            );
            if stream_tx.try_send(event).is_err() {
                tracing::warn!(
                    hang_probe = "ChunkToStreamSender::forward",
                    forwarded,
                    "hang_probe: forward send returned Err (receiver dropped)"
                );
                break;
            }
            let send_elapsed = send_start.elapsed();
            if send_elapsed > std::time::Duration::from_millis(50) {
                tracing::warn!(
                    hang_probe = "ChunkToStreamSender::forward",
                    forwarded,
                    send_elapsed_ms = send_elapsed.as_millis() as u64,
                    "hang_probe: forward send blocked >50ms"
                );
            }
        }
        tracing::info!(
            hang_probe = "ChunkToStreamSender::forward",
            forwarded,
            "hang_probe: forward end"
        );
        (forwarded, first_token_at)
    }
}
