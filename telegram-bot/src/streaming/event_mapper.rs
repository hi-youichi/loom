//! No-op event mapper.
//!
//! The streaming handler no longer displays intermediate act/tool phases.
//! This module is kept as a thin adapter to satisfy the agent run callback signature.

use std::sync::Arc;

use loom::AnyStreamEvent;
use tokio::sync::mpsc;

use crate::streaming::message_handler::StreamCommand;

pub(crate) struct StreamEventMapper {
    _tx: mpsc::Sender<StreamCommand>,
}

impl StreamEventMapper {
    pub(crate) fn new(tx: mpsc::Sender<StreamCommand>) -> Arc<Self> {
        Arc::new(Self { _tx: tx })
    }

    pub(crate) fn boxed_callback(self: &Arc<Self>) -> Box<dyn FnMut(AnyStreamEvent) + Send> {
        let _inner = Arc::clone(self);
        Box::new(move |_ev| {
            // Intentionally empty: no intermediate display.
        })
    }
}
