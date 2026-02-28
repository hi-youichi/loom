//! Agent run task: stream events to protocol envelopes and optional message store append.

use loom::{
    run_agent_with_options, AnyStreamEvent, EnvelopeState, Message, ProtocolEventEnvelope,
    RunCmd, RunError, RunOptions, StreamEvent,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Default event queue capacity (used by tests; production uses [`crate::app::RunConfig`]).
#[allow(dead_code)]
pub(super) const EVENT_QUEUE_CAPACITY: usize = 128;

/// Default append queue capacity (used by tests; production uses [`crate::app::RunConfig`]).
#[allow(dead_code)]
pub(super) const APPEND_QUEUE_CAPACITY: usize = 64;

/// Extracts the message list from a React stream event, if the event carries one.
fn react_event_messages(ev: &AnyStreamEvent) -> Option<&[Message]> {
    let AnyStreamEvent::React(react_ev) = ev else { return None };
    let messages = match react_ev {
        StreamEvent::Values(s) => &s.messages[..],
        StreamEvent::Updates { state: s, .. } => &s.messages[..],
        StreamEvent::Checkpoint(cp) => &cp.state.messages[..],
        _ => return None,
    };
    Some(messages)
}

/// Sends only the *new* messages (since last seen count) from a React event into the
/// append channel; when append channel is full, increments `dropped_appends` if provided.
fn forward_react_messages_to_append(
    ev: &AnyStreamEvent,
    append_tx: Option<&mpsc::Sender<(String, Message)>>,
    thread_id: Option<&String>,
    message_count: &Arc<Mutex<usize>>,
    dropped_appends: Option<&Arc<AtomicUsize>>,
) {
    let Some(atx) = append_tx else { return };
    let Some(tid) = thread_id else { return };
    let Some(messages) = react_event_messages(ev) else { return };
    let mut seen_count = match message_count.lock() {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!("message_count lock failed (poisoned?): {}", e);
            return;
        }
    };
    let start = *seen_count;
    if start >= messages.len() {
        return;
    }
    for msg in &messages[start..] {
        if atx.try_send((tid.clone(), msg.clone())).is_err() {
            if let Some(c) = dropped_appends {
                c.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    *seen_count = messages.len();
}

/// Handles a single stream event: forward new messages to append channel, convert to
/// protocol envelope and send to `tx`. Increments drop counters when queues are full.
fn process_run_stream_event(
    ev: AnyStreamEvent,
    state: &Arc<Mutex<EnvelopeState>>,
    tx: &mpsc::Sender<ProtocolEventEnvelope>,
    append_tx: Option<&mpsc::Sender<(String, Message)>>,
    thread_id: Option<&String>,
    message_count: &Arc<Mutex<usize>>,
    dropped_events: Option<&Arc<AtomicUsize>>,
    dropped_appends: Option<&Arc<AtomicUsize>>,
) {
    forward_react_messages_to_append(&ev, append_tx, thread_id, message_count, dropped_appends);

    let mut guard = match state.lock() {
        Ok(g) => g,
        Err(e) => {
            tracing::error!("envelope state lock failed (poisoned?): {}", e);
            return;
        }
    };
    let Ok(protocol_envelope) = ev.to_protocol_event(&mut *guard) else { return };
    if tx.try_send(protocol_envelope).is_err() {
        if let Some(c) = dropped_events {
            c.fetch_add(1, Ordering::Relaxed);
        }
        tracing::warn!(
            "event queue full, dropping stream event (receiver likely disconnected)"
        );
    }
}

/// Runs the agent in the current task. Returns result, envelope state, and drop counters.
pub(super) async fn run_agent_task(
    session_id: String,
    tx: mpsc::Sender<ProtocolEventEnvelope>,
    opts: RunOptions,
    cmd: RunCmd,
    initial_user_appended: bool,
    user_message_store: Option<Arc<dyn loom::UserMessageStore>>,
    thread_id: Option<String>,
    append_queue_capacity: usize,
) -> (
    Result<String, RunError>,
    Arc<Mutex<EnvelopeState>>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    let state = Arc::new(Mutex::new(EnvelopeState::new(session_id.clone())));
    let state_clone = state.clone();
    let dropped_events = Arc::new(AtomicUsize::new(0));
    let dropped_appends = Arc::new(AtomicUsize::new(0));

    let (append_tx, mut append_rx) = mpsc::channel::<(String, Message)>(append_queue_capacity);
    let message_count = Arc::new(Mutex::new(if initial_user_appended { 1 } else { 0 }));
    let append_handle = if let (Some(store), Some(_thread_id)) =
        (user_message_store.as_ref(), thread_id.as_ref())
    {
        let store = Arc::clone(store);
        Some(tokio::spawn(async move {
            while let Some((tid, msg)) = append_rx.recv().await {
                if let Err(e) = store.append(&tid, &msg).await {
                    tracing::warn!("user_message_store append: {}", e);
                }
            }
        }))
    } else {
        drop(append_rx);
        None
    };
    let append_tx_for_closure = append_handle.as_ref().map(|_| append_tx.clone());
    let append_tx_to_drop = append_handle.is_some().then_some(append_tx);
    let thread_id_closure = thread_id.clone();
    let message_count_clone = message_count.clone();
    let dropped_events_clone = dropped_events.clone();
    let dropped_appends_clone = dropped_appends.clone();

    let on_event = Box::new(move |ev: AnyStreamEvent| {
        process_run_stream_event(
            ev,
            &state_clone,
            &tx,
            append_tx_for_closure.as_ref(),
            thread_id_closure.as_ref(),
            &message_count_clone,
            Some(&dropped_events_clone),
            Some(&dropped_appends_clone),
        );
    });
    let result = run_agent_with_options(&opts, &cmd, Some(on_event)).await;
    drop(append_tx_to_drop);
    if let Some(h) = append_handle {
        let _ = h.await;
    }
    (result, state, dropped_events, dropped_appends)
}
