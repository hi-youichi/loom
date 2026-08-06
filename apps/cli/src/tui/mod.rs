//! Loom TUI — Interactive Terminal User Interface
//!
//! Provides an optional interactive TUI mode for Loom CLI.
//! Activated via `--interactive` / `-i` flag.
//!
//! ## Architecture
//!
//! Infrastructure layer: terminal init, event system, inline viewport, history
//! line insertion, ratatui rendering pipeline, and App main loop with full
//! component integration (PaneStack, Composer, StatusBar, etc.).

pub mod app;
pub mod agent;
pub mod approval;
pub mod composer;
pub mod diff;
pub mod event;
pub mod history;
pub mod history_cell;
pub mod job_control;
pub mod notification;
pub mod pane;
pub mod render;
pub mod selection;
pub mod spinner;
pub mod state;
pub mod status;
pub mod streaming;
pub mod terminal;
pub mod viewport;

// ── Core application ─────────────────────────────────────────────────────────
pub use app::App;
pub use event::{EventBroker, TuiEvent};
pub use terminal::{init, restore};

// ── Agent integration ────────────────────────────────────────────────────────
pub use agent::{AgentEvent, create_agent_channel, create_stream_callback};

// ── Approval ─────────────────────────────────────────────────────────────────
pub use approval::{ApprovalOverlay, ApprovalRequest, ApprovalResult};

// ── Composer (input box) ─────────────────────────────────────────────────────
pub use composer::Composer;

// ── Diff rendering ───────────────────────────────────────────────────────────
pub use diff::{DiffView, DiffPreview, DiffLine, DiffLineType};

// ── History rendering ────────────────────────────────────────────────────────
pub use history_cell::{HistoryCell, ToolStatus, SystemMessageStyle};

// ── Notifications ────────────────────────────────────────────────────────────
pub use notification::{NotificationManager, NotificationType};

// ── Pane system ──────────────────────────────────────────────────────────────
pub use pane::{PaneStack, PaneView, Handled, CtrlCAction};

// ── Rendering traits ─────────────────────────────────────────────────────────
pub use render::{Renderable, ColumnRenderable, FlexRenderable, InsetRenderable};

// ── Selection ────────────────────────────────────────────────────────────────
pub use selection::{SelectionList, SelectionItem};

// ── Spinner widget ───────────────────────────────────────────────────────────
pub use spinner::SpinnerWidget;

// ── State machine ────────────────────────────────────────────────────────────
pub use state::{AppState, AppEvent};

// ── Status bar ───────────────────────────────────────────────────────────────
pub use status::{StatusBar, AiStatus};

// ── Streaming output ─────────────────────────────────────────────────────────
pub use streaming::StreamingCell;