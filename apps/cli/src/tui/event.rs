//! Event system for the TUI.
//!
//! Provides:
//! - `TuiEvent` — unified event enum for key, paste, resize, draw, resume
//! - `EventBroker` — Arc-shared event dispatcher with pause/resume support
//! - `event_stream()` — wraps crossterm `EventStream` into a `TuiEvent` stream

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, KeyEvent, KeyModifiers, KeyCode};
use futures_util::stream::Stream;
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

/// Unified TUI event type.
#[derive(Debug, Clone)]
pub enum TuiEvent {
    /// Keyboard key press event.
    Key(KeyEvent),
    /// Bracketed paste content (text).
    Paste(String),
    /// Terminal resize event.
    Resize(u16, u16),
    /// Periodic draw tick (every ~100ms).
    Draw,
    /// Resume from suspend (^Z).
    Resume,
    /// Suspend request (Ctrl+Z).
    Suspend,
    /// Terminal window gained focus.
    FocusGained,
    /// Terminal window lost focus.
    FocusLost,
}

/// Arc-shared event dispatcher with pause/resume support.
///
/// When paused, events are silently dropped instead of queued.
/// This is useful during ^Z suspend or when a modal dialog is active.
#[derive(Clone)]
pub struct EventBroker {
    paused: Arc<AtomicBool>,
    tx: mpsc::UnboundedSender<TuiEvent>,
}

impl EventBroker {
    /// Create a new `EventBroker` and return it along with a receiving stream.
    pub fn new() -> (Self, impl Stream<Item = TuiEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let broker = Self {
            paused: Arc::new(AtomicBool::new(false)),
            tx,
        };
        let stream = UnboundedReceiverStream::new(rx);
        (broker, stream)
    }

    /// Pause event dispatching — events will be dropped.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }

    /// Resume event dispatching.
    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
    }

    /// Check if the broker is paused.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Send an event if not paused.
    pub fn send(&self, event: TuiEvent) {
        if !self.paused.load(Ordering::Relaxed) {
            let _ = self.tx.send(event);
        }
    }
}

/// Create a TuiEvent stream from crossterm's event stream.
///
/// The stream runs in a background tokio task, translating crossterm events
/// into `TuiEvent` variants and sending them through the `EventBroker`.
///
/// A periodic draw tick (~100ms) is also generated to drive rendering.
///
/// Returns a `JoinHandle` that can be cancelled when the application exits.
pub fn spawn_event_stream(broker: EventBroker) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut crossterm_stream = crossterm::event::EventStream::new();
        let mut draw_interval = tokio::time::interval(Duration::from_millis(100));
        // Skip the first immediate tick
        draw_interval.tick().await;

        loop {
            tokio::select! {
                Some(Ok(event)) = crossterm_stream.next() => {
                    match event {
                        Event::Key(key) => {
                            // Handle Ctrl+Z locally for suspend
                            if key.code == KeyCode::Char('z')
                                && key.modifiers.contains(KeyModifiers::CONTROL)
                            {
                                broker.send(TuiEvent::Suspend);
                                continue;
                            }
                            broker.send(TuiEvent::Key(key));
                        }
                        Event::Paste(text) => {
                            broker.send(TuiEvent::Paste(text));
                        }
                        Event::FocusGained => {
                            broker.send(TuiEvent::FocusGained);
                        }
                        Event::FocusLost => {
                            broker.send(TuiEvent::FocusLost);
                        }
                        Event::Resize(w, h) => {
                            broker.send(TuiEvent::Resize(w, h));
                        }
                        _ => {}
                    }
                }
                _ = draw_interval.tick() => {
                    broker.send(TuiEvent::Draw);
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_broker_send_recv() {
        let (broker, _stream) = EventBroker::new();
        broker.send(TuiEvent::Draw);
        // Can't easily test async stream in sync test, but at least no panic
        assert!(!broker.is_paused());
    }

    #[test]
    fn test_event_broker_pause() {
        let (broker, _stream) = EventBroker::new();
        assert!(!broker.is_paused());
        broker.pause();
        assert!(broker.is_paused());
        broker.resume();
        assert!(!broker.is_paused());
    }
}