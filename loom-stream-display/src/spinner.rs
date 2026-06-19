//! Simple animated spinner for stderr progress indication.
//!
//! Shows a single-line status bar that updates in-place using `\r` (carriage return).
//! Automatically degrades to static `eprintln!` when stderr is not a TTY.

use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const TICK_INTERVAL: Duration = Duration::from_millis(150);

/// Messages sent from the owner to the spinner background thread.
enum SpinnerMsg {
    Update(String),
    /// Update the context info (turn number, session start instant).
    SetContext { turn: u32, session_start: Instant },
    Finish,
}

/// Context for rendering elapsed time in the spinner label.
#[derive(Default)]
struct SpinnerContext {
    turn: u32,
    session_start: Option<Instant>,
}

/// Format elapsed seconds into a human-friendly string.
fn fmt_elapsed(dur: Duration) -> String {
    let secs = dur.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else {
        let m = secs / 60;
        let s = secs % 60;
        format!("{}m{}s", m, s)
    }
}

/// A trait for spinner-like objects, allowing polymorphism between active and no-op spinners.
pub trait SpinnerTrait: Send {
    fn update(&mut self, label: String);
    fn finish_box(self: Box<Self>);
}

/// A spinner that displays animated progress on stderr.
///
/// Usage:
/// ```ignore
/// let spinner = Spinner::new("Thinking...".to_string());
/// // ... do work ...
/// spinner.update("Executing tool: bash".to_string());
/// // ... more work ...
/// spinner.finish();
/// ```
pub struct Spinner {
    tx: mpsc::Sender<SpinnerMsg>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Spinner {
    /// Creates and starts a new spinner with the given initial label.
    ///
    /// If stderr is not a TTY (e.g., piped), this still accepts messages
    /// but renders them as static `eprintln!` lines instead of animated updates.
    pub fn new(label: String) -> Self {
        let (tx, rx) = mpsc::channel::<SpinnerMsg>();
        let is_tty = is_stderr_tty();

        let handle = std::thread::Builder::new()
            .name("loom-spinner".to_string())
            .spawn(move || {
                if is_tty {
                    run_tty_spinner(rx, &label);
                } else {
                    run_pipe_spinner(rx, &label);
                }
            })
            .expect("failed to spawn spinner thread");

        Self {
            tx,
            handle: Some(handle),
        }
    }

    /// Updates the spinner's status label.
    pub fn update(&self, label: String) {
        let _ = self.tx.send(SpinnerMsg::Update(label));
    }

    /// Sets the context for elapsed time rendering (turn number, session start).
    pub fn set_context(&self, turn: u32, session_start: Instant) {
        let _ = self.tx.send(SpinnerMsg::SetContext { turn, session_start });
    }

    /// Stops the spinner and clears the current line (TTY) or does nothing (pipe).
    pub fn finish(mut self) {
        let _ = self.tx.send(SpinnerMsg::Finish);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl SpinnerTrait for Spinner {
    fn update(&mut self, label: String) {
        Spinner::update(self, label);
    }
    fn finish_box(self: Box<Self>) {
        self.finish();
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        // Ensure the background thread is signaled on drop.
        let _ = self.tx.send(SpinnerMsg::Finish);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Runs the animated spinner loop for TTY stderr.
fn run_tty_spinner(rx: mpsc::Receiver<SpinnerMsg>, initial_label: &str) {
    let mut label = initial_label.to_string();
    let mut frame_idx = 0usize;
    let mut ctx = SpinnerContext::default();
    let spinner_start = Instant::now();

    let display_line = |label: &str, ctx: &SpinnerContext, spinner_start: Instant, frame_idx: usize| {
        let elapsed = spinner_start.elapsed();
        let suffix = build_elapsed_suffix(ctx, elapsed);
        let display = format!("{}{}", label, suffix);
        let stderr = std::io::stderr();
        let mut stderr_lock = stderr.lock();
        let _ = write!(
            stderr_lock,
            "\r{} {}",
            SPINNER_FRAMES[frame_idx % SPINNER_FRAMES.len()],
            truncate_to_terminal_width(&display, 4)
        );
        let _ = stderr_lock.flush();
    };

    // Initial display
    display_line(&label, &ctx, spinner_start, 0);

    loop {
        match rx.recv_timeout(TICK_INTERVAL) {
            Ok(SpinnerMsg::Update(new_label)) => {
                label = new_label;
                frame_idx = (frame_idx + 1) % SPINNER_FRAMES.len();
                display_line(&label, &ctx, spinner_start, frame_idx);
            }
            Ok(SpinnerMsg::SetContext { turn, session_start }) => {
                ctx.turn = turn;
                ctx.session_start = Some(session_start);
                display_line(&label, &ctx, spinner_start, frame_idx);
            }
            Ok(SpinnerMsg::Finish) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                let stderr = std::io::stderr();
                let mut stderr_lock = stderr.lock();
                let _ = write!(stderr_lock, "\r{}", " ".repeat(120));
                let _ = write!(stderr_lock, "\r");
                let _ = stderr_lock.flush();
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                frame_idx = (frame_idx + 1) % SPINNER_FRAMES.len();
                display_line(&label, &ctx, spinner_start, frame_idx);
            }
        }
    }
}

/// Build the elapsed-time suffix for the spinner label.
///
/// For thinking: `思考中... (第 N 轮, 共 Xs)` or `思考中... (Xs)` if first turn.
/// For tool execution: `执行工具: bash ... (Xs)`.
fn build_elapsed_suffix(ctx: &SpinnerContext, elapsed: Duration) -> String {
    let elapsed_str = fmt_elapsed(elapsed);
    if ctx.turn > 0 {
        if let Some(session_start) = ctx.session_start {
            let total = fmt_elapsed(session_start.elapsed());
            format!(" (第 {} 轮, 共 {})", ctx.turn, total)
        } else {
            format!(" (第 {} 轮, {})", ctx.turn, elapsed_str)
        }
    } else {
        format!(" ({})", elapsed_str)
    }
}

/// Runs a static spinner for piped (non-TTY) stderr.
fn run_pipe_spinner(rx: mpsc::Receiver<SpinnerMsg>, initial_label: &str) {
    // Just print the initial label once
    eprintln!("  {}", initial_label);

    loop {
        match rx.recv() {
            Ok(SpinnerMsg::Update(label)) => {
                eprintln!("  {}", label);
            }
            Ok(SpinnerMsg::SetContext { .. }) => {}
            Ok(SpinnerMsg::Finish) | Err(_) => return,
        }
    }
}

/// Truncates a string to fit within `margin` chars of the terminal width.
fn truncate_to_terminal_width(s: &str, margin: usize) -> String {
    let width = get_terminal_width();
    let max = if width > margin { width - margin } else { 80 };
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 3).collect();
        format!("{}...", truncated)
    }
}

/// Returns terminal width, defaulting to 80 on failure.
fn get_terminal_width() -> usize {
    crate::terminal::get_terminal_width()
}

/// Checks if stderr is connected to a TTY.
fn is_stderr_tty() -> bool {
    crate::terminal::is_stderr_tty()
}

/// A no-op spinner that does nothing. Used when spinning is disabled (quiet mode).
pub struct NoopSpinner;

impl NoopSpinner {
    pub fn new(_label: String) -> Self {
        Self
    }
    pub fn update(&self, _label: String) {}
    pub fn finish(self) {}
}

impl SpinnerTrait for NoopSpinner {
    fn update(&mut self, _label: String) {}
    fn finish_box(self: Box<Self>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_to_terminal_width_short_string_unchanged() {
        let result = truncate_to_terminal_width("hello", 2);
        assert_eq!(result, "hello");
    }

    #[test]
    fn truncate_to_terminal_width_long_string_truncated() {
        // Use a string long enough to exceed any terminal width (e.g. 10000 chars)
        let long: String = "x".repeat(10000);
        let result = truncate_to_terminal_width(&long, 2);
        assert!(result.ends_with("..."), "expected truncation, got: {} chars", result.len());
        assert!(result.len() < long.len());
    }

    #[test]
    fn spinner_msg_send_and_receive() {
        let (tx, rx) = mpsc::channel::<SpinnerMsg>();
        tx.send(SpinnerMsg::Update("test".to_string())).unwrap();
        match rx.recv().unwrap() {
            SpinnerMsg::Update(label) => assert_eq!(label, "test"),
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn noop_spinner_does_not_crash() {
        let spinner = NoopSpinner::new("test".to_string());
        spinner.update("updated".to_string());
        spinner.finish();
    }
}