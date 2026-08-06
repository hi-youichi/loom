//! TUI application main loop.
//!
//! The `App` struct owns the terminal, event system, viewport, pane stack,
//! composer, status bar, and agent event channel. It runs a `tokio::select!`
//! loop that handles TUI events, agent events, and job control (^Z suspend),
//! driving the ratatui-based rendering pipeline.

use std::pin::Pin;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use futures_util::stream::Stream;
use futures_util::StreamExt;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::Widget;
use ratatui::buffer::Buffer;
use tokio::sync::mpsc;

use super::agent::{AgentEvent, create_agent_channel};
use super::composer::{Composer, ComposerAction};
use super::event::{spawn_event_stream, EventBroker, TuiEvent};
use super::history_cell::{HistoryCell, SystemMessageStyle, ToolStatus};
use super::notification::{NotificationManager, NotificationType};
use super::pane::{CtrlCAction, Handled, PaneStack};
use super::render::Renderable;
use super::state::{AppEvent, AppState};
use super::status::{AiStatus, StatusBar};
use super::streaming::StreamingCell;
use super::terminal::{self, TuiTerminal};

// ---------------------------------------------------------------------------
// FocusState
// ---------------------------------------------------------------------------

/// Tracks whether the terminal window is focused (for desktop notifications).
#[derive(Debug, Clone)]
struct FocusState {
    focused: bool,
}

impl FocusState {
    fn new() -> Self {
        Self { focused: true }
    }
}

// ---------------------------------------------------------------------------
// RenderAsWidget — bridges our Renderable trait to ratatui's Widget trait
// ---------------------------------------------------------------------------

/// Wraps a `&dyn Renderable` so it can be rendered via `frame.render_widget()`.
struct RenderAsWidget<'a>(&'a dyn super::render::Renderable);

impl Widget for RenderAsWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.0.render(area, buf);
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

/// TUI application — main loop, state management, and component orchestration.
pub struct App {
    // ── Infrastructure ────────────────────────────────────────────────────
    /// Terminal manager (owns ratatui terminal, viewport, pending history)
    terminal: TuiTerminal,
    /// Event broker (for pause/resume of the event stream)
    event_broker: EventBroker,
    /// Event stream from crossterm
    event_stream: Pin<Box<dyn Stream<Item = TuiEvent> + Send>>,
    /// Handle to the background event stream task (cancelled on cleanup)
    event_task: Option<tokio::task::JoinHandle<()>>,

    // ── Application state ─────────────────────────────────────────────────
    /// Application state machine
    state: AppState,
    /// Whether the app is still running
    running: bool,

    // ── Conversation history ──────────────────────────────────────────────
    /// Completed conversation turns (rendered as history cells)
    history: Vec<HistoryCell>,
    /// Currently streaming agent response (None when idle)
    active_cell: Option<StreamingCell>,

    // ── Input / Interaction ───────────────────────────────────────────────
    /// Text input composer (base input field)
    composer: Composer,
    /// Stack of overlay panels (approval, selection, etc.)
    pane_stack: PaneStack,

    // ── Status ────────────────────────────────────────────────────────────
    /// Status bar at the bottom of the TUI
    status_bar: StatusBar,

    // ── Agent integration ─────────────────────────────────────────────────
    /// Sender for agent streaming events (used to spawn agent tasks)
    agent_tx: Option<mpsc::Sender<AgentEvent>>,
    /// Receiver for agent streaming events (None when no agent is running)
    agent_rx: Option<mpsc::Receiver<AgentEvent>>,

    // ── Notifications ─────────────────────────────────────────────────────
    /// Desktop notification manager
    notification_manager: NotificationManager,
    /// Terminal focus state (for notification suppression)
    focus_state: FocusState,

    // ── Job control (^Z) ──────────────────────────────────────────────────
    #[cfg(unix)]
    /// SIGTSTP suspend/resume manager
    job_control: super::job_control::JobControl,
}

impl App {
    /// Create a new TUI application.
    ///
    /// Does NOT initialize the terminal — call `run()` for that.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (broker, stream) = EventBroker::new();
        let terminal = TuiTerminal::new()?;

        Ok(Self {
            terminal,
            event_broker: broker,
            event_stream: Box::pin(stream),
            event_task: None,
            state: AppState::Idle,
            running: true,
            history: Vec::new(),
            active_cell: None,
            composer: Composer::new(),
            pane_stack: PaneStack::new(),
            status_bar: StatusBar::new(),
            agent_rx: None,
            agent_tx: None,
            notification_manager: NotificationManager::new(true),
            focus_state: FocusState::new(),
            #[cfg(unix)]
            job_control: super::job_control::JobControl::new(),
        })
    }

    /// Run the TUI application main loop.
    ///
    /// Initializes the terminal, sets up signal handlers, spawns the event
    /// stream, and enters the event loop. Returns when the user exits.
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Initialize terminal
        terminal::init()?;
        terminal::flush_input_buffer()?;

        // Update terminal now that we're in raw mode
        self.terminal = TuiTerminal::new()?;

        // Set up ^Z job control signal handler
        #[cfg(unix)]
        super::job_control::setup_signal_handler()?;

        // Spawn event stream
        self.event_task = Some(spawn_event_stream(self.event_broker.clone()));

        // Welcome message
        self.history.push(HistoryCell::system_message(
            " Loom TUI — 交互式 AI 编程助手 | Ctrl+D 退出 · Ctrl+C 中断".into(),
            SystemMessageStyle::Info,
        ));
        self.state = AppState::Idle;
        self.status_bar.set_status(AiStatus::Idle);

        // Render initial state
        self.render();

        // Main event loop
        while self.running {
            // ── Agent branch (always present, may be pending) ──────────
            let agent_fut = async {
                self.agent_rx.as_mut()?.recv().await
            };

            tokio::select! {
                // ── TUI events (keyboard, resize, paste, draw tick) ────
                Some(event) = self.event_stream.next() => {
                    self.handle_event(event).await;
                }

                // ── Agent streaming events ──────────────────────────────
                Some(agent_event) = agent_fut => {
                    self.handle_agent_event(agent_event);
                }
            }
        }

        // Restore terminal
        self.cleanup()?;
        Ok(())
    }

    // ── Event handling ───────────────────────────────────────────────────

    /// Handle a single TUI event.
    async fn handle_event(&mut self, event: TuiEvent) {
        match event {
            TuiEvent::Key(key) => self.handle_key(key),
            TuiEvent::Resize(w, h) => {
                self.terminal.viewport_mut().handle_resize(w, h);
                self.render();
            }
            TuiEvent::Draw => {
                // Advance spinner animation on draw ticks
                self.status_bar.tick();
                self.render();
            }
            TuiEvent::Suspend => {
                #[cfg(unix)]
                self.handle_suspend().await;
                #[cfg(not(unix))]
                self.render();
            }
            TuiEvent::FocusGained => {
                self.focus_state.focused = true;
            }
            TuiEvent::FocusLost => {
                self.focus_state.focused = false;
            }
            TuiEvent::Resume => {
                self.status_bar.set_message(Some("已恢复 (fg)".into()));
                self.status_bar.set_status(AiStatus::Idle);
                self.render();
            }
            TuiEvent::Paste(text) => {
                // Insert pasted text into the composer
                self.composer.insert_text(&text);
                self.render();
            }
        }
    }

    /// Handle a key event.
    ///
    /// Global keys (Ctrl+D, Ctrl+C, Ctrl+L) are handled first, then
    /// overlay panels, then the composer.
    fn handle_key(&mut self, key: KeyEvent) {
        // ── Global keys ─────────────────────────────────────────────────
        match key.code {
            // Ctrl+D: exit
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.running = false;
                return;
            }
            // Ctrl+C: cancel/interrupt
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Check if an overlay panel wants to handle Ctrl+C first
                if self.pane_stack.depth() > 0 {
                    match self.pane_stack.handle_ctrl_c() {
                        CtrlCAction::Handled | CtrlCAction::Cancel => {
                            self.render();
                            return;
                        }
                        CtrlCAction::NotHandled => {}
                    }
                }
                self.handle_interrupt();
                return;
            }
            // Ctrl+L: clear screen (clear history, keep welcome)
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.history.clear();
                self.history.push(HistoryCell::system_message(
                    " 屏幕已清除 ".into(),
                    SystemMessageStyle::Info,
                ));
                self.render();
                return;
            }
            _ => {}
        }

        // ── Overlay panels ──────────────────────────────────────────────
        // If there are active overlay panels, route the key to the top one.
        if self.pane_stack.depth() > 0 {
            let handled = self.pane_stack.handle_key_event(key);

            // If the panel was cancelled (Ctrl+C from overlay), clear state
            // Check if the overlay is complete and pop it
            // handle_key_event already does cleanup_completed() internally

            if handled == Handled::Handled {
                self.render();
                return;
            }
        }

        // ── Composer input ──────────────────────────────────────────────
        match self.composer.handle_key(key) {
            ComposerAction::Submit(content) => {
                self.handle_submit(content);
            }
            ComposerAction::Continue => {
                self.render();
            }
        }
    }

    /// Handle Ctrl+C interrupt.
    fn handle_interrupt(&mut self) {
        // Transition state
        if let Ok(new_state) = self.state.transition(AppEvent::Interrupt) {
            self.state = new_state;
        }

        self.status_bar.set_status(AiStatus::Idle);
        self.status_bar.set_message(Some("已中断".into()));

        // Add interrupt message to history
        self.history.push(HistoryCell::system_message(
            " [中断] 操作已取消".into(),
            SystemMessageStyle::Warning,
        ));

        // Drop active streaming cell
        self.active_cell = None;

        // Drop agent channel (stops agent task)
        self.agent_rx = None;

        self.render();
    }

    /// Handle user submit — send the input to the agent.
    fn handle_submit(&mut self, content: String) {
        // Transition to Submitting state
        if let Ok(new_state) = self.state.transition(AppEvent::Submit) {
            self.state = new_state;
        }

        // Add user message to history
        self.history.push(HistoryCell::user_message(content.clone()));

        // Create agent event channel
        let (agent_tx, agent_rx) = create_agent_channel();
        self.agent_tx = Some(agent_tx);
        self.agent_rx = Some(agent_rx);

        // Create streaming cell for the response
        self.active_cell = Some(StreamingCell::new());

        // Update status
        self.status_bar.set_status(AiStatus::Thinking);
        self.status_bar.set_message(Some("正在思考...".into()));

        // Note: The actual agent spawning will be done by the caller
        // (e.g., the CLI main loop) which provides the agent_tx sender.
        // For now, the agent_rx is ready to receive events.

        self.render();
    }

    /// Handle a suspend request (^Z).
    #[cfg(unix)]
    async fn handle_suspend(&mut self) {
        // Pause event stream
        self.event_broker.pause();

        // Suspend in a scoped thread (blocks on SIGTSTP, then re-inits terminal)
        std::thread::scope(|s| {
            let jc = &self.job_control;
            s.spawn(|| {
                let _ = jc.suspend();
            });
        });

        // Reinitialize terminal after resume
        if let Ok(new_terminal) = TuiTerminal::new() {
            self.terminal = new_terminal;
        }

        // Resume event stream
        self.event_broker.resume();

        // Update status
        self.status_bar.set_status(AiStatus::Idle);
        self.status_bar.set_message(Some("已恢复".into()));

        self.render();
    }

    // ── Agent event handling ─────────────────────────────────────────────

    /// Handle an agent streaming event.
    ///
    /// Follows the event handling flow from agent-integration.md §3.2.
    fn handle_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta(content) => {
                // Update or create streaming cell
                if let Some(active) = &mut self.active_cell {
                    active.append_text(&content);
                } else {
                    let mut cell = StreamingCell::new();
                    cell.append_text(&content);
                    self.active_cell = Some(cell);
                }

                // Update state
                if let Ok(new_state) = self.state.transition(AppEvent::Processing) {
                    self.state = new_state;
                }
                self.status_bar.set_status(AiStatus::Thinking);
                self.render();
            }

            AgentEvent::ReasoningDelta(_content) => {
                // AI is reasoning — show thinking state
                if let Ok(new_state) = self.state.transition(AppEvent::Processing) {
                    self.state = new_state;
                }
                self.status_bar.set_status(AiStatus::Thinking);
                self.render();
            }

            AgentEvent::ToolCall { name, arguments, .. } => {
                // Show tool call in status
                self.status_bar.set_status(AiStatus::Executing);
                self.status_bar.set_message(Some(format!("调用: {}", name)));

                // Add a tool call cell to history
                self.history.push(HistoryCell::tool_call(
                    name,
                    arguments,
                    None,
                    ToolStatus::Running,
                ));
                self.render();
            }

            AgentEvent::ToolStart { name, .. } => {
                self.status_bar.set_status(AiStatus::Executing);
                self.status_bar.set_message(Some(format!("执行: {}", name)));
                self.render();
            }

            AgentEvent::ToolOutput { content, .. } => {
                // Update the last tool call in history with output
                if let Some(HistoryCell::ToolCall { result, .. }) = self.history.last_mut() {
                    *result = Some(content.clone());
                }
                self.render();
            }

            AgentEvent::ToolEnd {
                name, result, is_error, ..
            } => {
                // Update the last tool call's status
                if let Some(HistoryCell::ToolCall { status, result: res, .. }) =
                    self.history.last_mut()
                {
                    *status = if is_error {
                        ToolStatus::Failed
                    } else {
                        ToolStatus::Completed
                    };
                    *res = Some(result);
                }

                self.status_bar.set_status(AiStatus::Thinking);
                self.status_bar
                    .set_message(Some(format!("{} 完成", name)));
                self.render();
            }

            AgentEvent::Completed => {
                // Move streaming cell to history as an assistant message
                if let Some(active) = self.active_cell.take() {
                    let content = active.finish();
                    if !content.is_empty() {
                        self.history
                            .push(HistoryCell::assistant_message(content));
                    }
                }

                // Transition back to idle
                if let Ok(new_state) = self.state.transition(AppEvent::Completed) {
                    self.state = new_state;
                }
                self.status_bar.set_status(AiStatus::Idle);
                self.status_bar.set_message(None);

                // Drop agent channel
                self.agent_rx = None;

                // Send desktop notification if terminal is not focused
                if !self.focus_state.focused {
                    let _ = self
                        .notification_manager
                        .notify(NotificationType::ReplyComplete);
                }

                self.render();
            }

            AgentEvent::Error(e) => {
                // Add error to history
                self.history.push(HistoryCell::system_message(
                    format!(" 错误: {} ", e),
                    SystemMessageStyle::Error,
                ));

                // Update state
                if let Ok(new_state) = self.state.transition(AppEvent::Error) {
                    self.state = new_state;
                }
                self.status_bar.set_status(AiStatus::Error);
                self.status_bar.set_message(Some(e));

                // Drop active streaming cell
                self.active_cell = None;

                // Drop agent channel
                self.agent_rx = None;

                // Send desktop notification
                if !self.focus_state.focused {
                    let _ = self
                        .notification_manager
                        .notify(NotificationType::Error);
                }

                self.render();
            }
        }
    }

    // ── Rendering ────────────────────────────────────────────────────────

    /// Render the current state to the terminal using ratatui.
    fn render(&mut self) {
        let viewport_height = self.terminal.viewport().height();

        let _ = self.terminal.draw_ratatui(viewport_height, |frame| {
            let area = frame.area();

            // ── Layout ──────────────────────────────────────────────────
            // Split into: history (fill) | active/pane (min) | status (2)
            let chunks = Layout::vertical([
                Constraint::Fill(1),
                Constraint::Min(3),
                Constraint::Length(2),
            ])
            .split(area);

            let history_area = chunks[0];
            let middle_area = chunks[1];
            let status_area = chunks[2];

            // ── History rendering ───────────────────────────────────────
            Self::render_history(&self.history, history_area, frame);

            // ── Middle area: active cell or pane stack ──────────────────
            if self.pane_stack.depth() > 0 {
                // Render the overlay panel (top of stack)
                frame.render_widget(
                    RenderAsWidget(&self.pane_stack),
                    middle_area,
                );
            } else if let Some(active) = &self.active_cell {
                // Render the active streaming cell
                frame.render_widget(RenderAsWidget(active), middle_area);
            } else {
                // Render the composer as the base input
                frame.render_widget(RenderAsWidget(&self.composer), middle_area);
            }

            // ── Status bar ─────────────────────────────────────────────
            frame.render_widget(RenderAsWidget(&self.status_bar), status_area);
        });
    }

    /// Render history cells into the given area, bottom-aligned.
    fn render_history(
        history: &[HistoryCell],
        area: Rect,
        frame: &mut ratatui::Frame,
    ) {
        if history.is_empty() || area.height == 0 {
            return;
        }

        // Calculate total desired height of all cells
        let mut total_height: u16 = 0;
        let mut cell_heights: Vec<u16> = Vec::with_capacity(history.len());
        for cell in history.iter().rev() {
            let h = cell.desired_height(area.width).min(area.height);
            cell_heights.push(h);
            total_height += h;
            if total_height >= area.height {
                break;
            }
        }

        // Render from bottom, most recent first
        let mut y_offset = area.y + area.height;
        for (i, cell) in history.iter().rev().enumerate() {
            if i >= cell_heights.len() {
                break;
            }
            let h = cell_heights[i];
            if h == 0 || y_offset < area.y + h {
                break;
            }
            y_offset -= h;
            let cell_area = Rect::new(area.x, y_offset, area.width, h);
            frame.render_widget(RenderAsWidget(cell), cell_area);
        }
    }

    // ── Cleanup ──────────────────────────────────────────────────────────

    /// Clean up and restore terminal state.
    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.state = AppState::Exiting;
        self.agent_rx = None;

        // Cancel the background event stream task
        if let Some(handle) = self.event_task.take() {
            handle.abort();
        }

        terminal::restore()?;
        Ok(())
    }

    // ── Public accessors ─────────────────────────────────────────────────

    /// Get the current application state.
    pub fn state(&self) -> AppState {
        self.state
    }

    /// Check if the app is still running.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Get a mutable reference to the agent event receiver.
    ///
    /// The caller can use this to set up the agent channel before spawning
    /// the agent task.
    pub fn agent_rx(&mut self) -> &mut Option<mpsc::Receiver<AgentEvent>> {
        &mut self.agent_rx
    }

    /// Get the history of conversation cells.
    pub fn history(&self) -> &[HistoryCell] {
        &self.history
    }

    /// Get the current status bar.
    pub fn status_bar(&self) -> &StatusBar {
        &self.status_bar
    }

    /// Get a mutable reference to the status bar.
    pub fn status_bar_mut(&mut self) -> &mut StatusBar {
        &mut self.status_bar
    }

    /// Get the terminal reference.
    pub fn terminal(&self) -> &TuiTerminal {
        &self.terminal
    }
}