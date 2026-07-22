//! StreamEventSink: lightweight adapter that implements [`StreamSink`] and forwards
//! streamed `MessageChunk`s directly to a `mpsc::Sender<StreamEvent<S>>`.
//!
//! This replaces the previous `ChunkToStreamSender` adapter that required an extra
//! intermediate `mpsc::channel::<MessageChunk>(128)` and a separate `forward()` task.
//! With `StreamEventSink`, the LLM implementation calls `sink.try_send_message(chunk, node_id)`
//! directly inside its SSE parsing loop. There is **no** intermediate channel, **no**
//! forwarder task, and **no** `.await` on a send inside the LLM — the LLM only does a
//! single `try_send` per chunk.
//!
//! # Why this design
//!
//! - The LLM streaming hot path used to be:
//!   `LLM (await send) → chunk_rx (cap 128) → forward task (try_send) → stream_tx`.
//!   If `stream_tx` was full or its receiver slow, `forward` would `try_send` Err and
//!   `break`, leaving `chunk_rx` undrained, which then backed up and stalled the LLM's
//!   `await send`.
//! - The new path is `LLM (try_send) → stream_tx`. The LLM never awaits a send; the
//!   downstream consumer is the only backpressure point.
//!
//! # `first_chunk_at`
//!
//! `try_send_message` returns `Some(Instant)` exactly once (on the very first chunk),
//! so the caller can populate `LlmResponse::first_chunk_at` for prefill/decode timing.

use std::fmt::Debug;
use std::sync::Mutex;

use crate::types::message::{MessageChunk, StreamSink};
use tokio::sync::mpsc;

use crate::types::metadata::StreamMetadata;
use crate::types::stream_event::StreamEvent;

/// Sink that converts `MessageChunk`s into `StreamEvent::Messages` and forwards them
/// to a `mpsc::Sender<StreamEvent<S>>`.
///
/// Implements [`StreamSink`] so it can be passed directly to
/// Streaming sink called from LLM clients during `invoke_stream`.
///
/// Created per LLM call (cheap: just two `clone()`s and one `Mutex`). Cheap to drop.
pub struct StreamEventSink<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    stream_tx: mpsc::Sender<StreamEvent<S>>,
    namespace: Option<String>,
    first_chunk_at: Mutex<Option<std::time::Instant>>,
}

impl<S> StreamEventSink<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    /// Create a new sink that forwards to `stream_tx`. `namespace` is attached to every
    /// outgoing event for subgraph routing (pass `None` for top-level graphs).
    pub fn new(stream_tx: mpsc::Sender<StreamEvent<S>>, namespace: Option<String>) -> Self {
        Self {
            stream_tx,
            namespace,
            first_chunk_at: Mutex::new(None),
        }
    }

    /// Returns `true` if at least one chunk has been forwarded through this sink.
    /// Useful for tests and for callers that want to inspect state without moving
    /// out of the `Mutex`.
    pub fn has_emitted(&self) -> bool {
        self.first_chunk_at.lock().unwrap().is_some()
    }
}

impl<S> StreamSink for StreamEventSink<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    fn try_send_message(&self, chunk: MessageChunk, node_id: &str) -> Option<std::time::Instant> {
        let event = StreamEvent::Messages {
            chunk,
            metadata: StreamMetadata {
                loom_node: node_id.to_string(),
                namespace: self.namespace.clone(),
            },
        };
        // Non-blocking send: drop chunk silently if downstream is full / disconnected.
        let _ = self.stream_tx.try_send(event);

        // First-chunk timing: return Some(Instant) exactly once.
        let mut guard = self
            .first_chunk_at
            .lock()
            .expect("first_chunk_at mutex poisoned");
        if guard.is_none() {
            *guard = Some(std::time::Instant::now());
            *guard
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::message::MessageChunk;
    use std::sync::Arc;

    #[derive(Clone, Debug)]
    struct TestState;

    #[test]
    fn first_chunk_emits_event_and_returns_some_instant() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<TestState>>(16);
        let sink = StreamEventSink::new(tx, None);

        let now = sink
            .try_send_message(MessageChunk::message("hello"), "think")
            .expect("first chunk should return Instant");

        // Channel should now have one event
        let ev = rx.try_recv().expect("event should be sent");
        match ev {
            StreamEvent::Messages { chunk, metadata } => {
                assert_eq!(chunk.content, "hello");
                assert_eq!(chunk.kind, crate::types::message::MessageChunkKind::Message);
                assert_eq!(metadata.loom_node, "think");
                assert!(metadata.namespace.is_none());
            }
            other => panic!("expected Messages, got {:?}", other),
        }
        assert!(sink.has_emitted());
        // We don't assert the instant value is `now` because try_send_message captures
        // its own Instant internally.
        let _ = now;
    }

    #[test]
    fn second_chunk_returns_none() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<TestState>>(16);
        let sink = StreamEventSink::new(tx, None);

        let first = sink.try_send_message(MessageChunk::message("a"), "think");
        let second = sink.try_send_message(MessageChunk::message("b"), "think");
        assert!(first.is_some());
        assert!(second.is_none());

        let _ = rx.try_recv();
        let _ = rx.try_recv();
    }

    #[test]
    fn thinking_chunk_kind_is_preserved() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<TestState>>(16);
        let sink = StreamEventSink::new(tx, None);

        let _ = sink.try_send_message(MessageChunk::thinking("reasoning"), "think");
        let ev = rx.try_recv().unwrap();
        match ev {
            StreamEvent::Messages { chunk, .. } => {
                assert!(chunk.is_thinking());
                assert_eq!(chunk.content, "reasoning");
            }
            other => panic!("expected Messages, got {:?}", other),
        }
    }

    #[test]
    fn namespace_is_attached() {
        let (tx, mut rx) = mpsc::channel::<StreamEvent<TestState>>(16);
        let sink = StreamEventSink::new(tx, Some("sub".to_string()));

        let _ = sink.try_send_message(MessageChunk::message("hi"), "think");
        let ev = rx.try_recv().unwrap();
        match ev {
            StreamEvent::Messages { metadata, .. } => {
                assert_eq!(metadata.namespace.as_deref(), Some("sub"));
            }
            other => panic!("expected Messages, got {:?}", other),
        }
    }

    #[test]
    fn downstream_drop_does_not_panic() {
        let (tx, rx) = mpsc::channel::<StreamEvent<TestState>>(1);
        let sink = StreamEventSink::new(tx, None);
        drop(rx);

        // try_send_message should not panic even if the receiver is gone.
        let result = sink.try_send_message(MessageChunk::message("dropped"), "think");
        // First chunk still returns Some(Instant) — we record first_chunk_at
        // before discovering the send failed.
        assert!(result.is_some());
    }

    #[test]
    fn concurrent_senders_only_return_first_instant_once() {
        let (tx, _rx) = mpsc::channel::<StreamEvent<TestState>>(128);
        let sink = Arc::new(StreamEventSink::new(tx, None));
        let mut handles = vec![];

        for _ in 0..8 {
            let sink = Arc::clone(&sink);
            handles.push(std::thread::spawn(move || {
                let mut got_some = false;
                for _ in 0..50 {
                    if sink
                        .try_send_message(MessageChunk::message("x"), "think")
                        .is_some()
                    {
                        got_some = true;
                    }
                }
                got_some
            }));
        }

        let mut total_first = 0;
        for h in handles {
            if h.join().unwrap() {
                total_first += 1;
            }
        }
        // Exactly one thread should observe the very first chunk across all threads.
        assert_eq!(total_first, 1);
    }
}
