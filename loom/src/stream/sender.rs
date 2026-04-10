use super::super::{MessageChunk, StreamEvent, StreamMetadata};
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
        while let Some(chunk) = chunk_rx.recv().await {
            if first_token_at.is_none() {
                first_token_at = Some(std::time::Instant::now());
            }
            forwarded += 1;
            let event = StreamEvent::Messages {
                chunk,
                metadata: StreamMetadata {
                    loom_node: node_id.clone(),
                    namespace: namespace.clone(),
                },
            };
            let _ = stream_tx.send(event).await;
        }
        (forwarded, first_token_at)
    }
}