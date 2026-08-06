//! Terminal initialization, restoration, and inline viewport rendering.
//!
//! Provides:
//! - `init()` — enable raw mode, bracketed paste, focus events, panic hook
//! - `restore()` — cleanly restore terminal to original state
//! - `TuiTerminal` — manages inline viewport, pending history flush, and
//!   ratatui rendering pipeline

use std::io::{stdout, Write};

use crossterm::{
    cursor::{Hide, Show},
    event::{
        DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, size, Clear, ClearType},
};
use ratatui::{backend::CrosstermBackend, Frame, Terminal};

use super::history::{HistoryLineWrapPolicy, PendingHistory};
use super::viewport::Viewport;

/// Initialize terminal for TUI mode.
///
/// Must be called before any TUI rendering. Caller MUST call `restore()` on
/// exit (or rely on panic hook to do so).
pub fn init() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    execute!(stdout(), EnableBracketedPaste)?;
    execute!(stdout(), EnableFocusChange)?;
    execute!(stdout(), Hide)?;
    set_panic_hook();
    Ok(())
}

/// Restore terminal to original state.
///
/// Safe to call multiple times; subsequent calls are no-ops after the first
/// successful restore.
pub fn restore() -> Result<(), Box<dyn std::error::Error>> {
    execute!(stdout(), DisableBracketedPaste)?;
    execute!(stdout(), DisableFocusChange)?;
    execute!(stdout(), Show)?;
    // Clear any remaining inline content
    let _ = execute!(stdout(), Clear(ClearType::FromCursorDown));
    disable_raw_mode()?;
    Ok(())
}

/// Install a panic hook that restores the terminal before the default handler.
fn set_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore();
        prev(panic_info);
    }));
}

/// Flush any buffered key events that arrived before init().
///
/// Call this after `init()` to discard stray keystrokes typed before the
/// TUI was ready.
pub fn flush_input_buffer() -> Result<(), Box<dyn std::error::Error>> {
    // Drain stdin using non-blocking read; this is best-effort.
    // On Unix, we use select() or just attempt reads with a small buffer.
    use std::io::Read;
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let stdin_fd = std::io::stdin().as_raw_fd();
        let mut set: libc::fd_set = unsafe { std::mem::zeroed() };
        unsafe {
            libc::FD_ZERO(&mut set);
            libc::FD_SET(stdin_fd, &mut set);
        }
        let mut timeout = libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        loop {
            let mut read_set = set;
            let ret = unsafe {
                libc::select(
                    stdin_fd + 1,
                    &mut read_set,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut timeout,
                )
            };
            if ret <= 0 {
                break;
            }
            let mut buf = [0u8; 256];
            let _ = std::io::stdin().read(&mut buf);
        }
    }
    #[cfg(not(unix))]
    {
        // Non-Unix: best-effort, drain what we can
        let mut buf = [0u8; 256];
        let _ = std::io::stdin().read(&mut buf);
    }
    Ok(())
}

/// Probe the current cursor position via DSR (Device Status Report).
///
/// Returns `(row, col)` as 1-based terminal coordinates.
/// Falls back to `(0, 0)` if the terminal doesn't respond.
pub fn probe_cursor_position() -> (u16, u16) {
    // Use crossterm's built-in cursor position query
    match crossterm::cursor::position() {
        Ok((col, row)) => (row, col),
        Err(_) => (0, 0),
    }
}

/// Get terminal size as `(width, height)`.
pub fn terminal_size() -> (u16, u16) {
    size().unwrap_or((80, 24))
}

/// TUI terminal manager — owns the inline viewport, pending history, and
/// ratatui rendering pipeline.
pub struct TuiTerminal {
    /// Ratatui terminal for rich rendering (initialized in `new()`)
    ratatui_terminal: Option<Terminal<CrosstermBackend<std::io::Stdout>>>,
    /// Inline viewport manager
    viewport: Viewport,
    /// Pending history lines to flush before each draw
    pending_history: PendingHistory,
    /// Whether the terminal has been initialized
    initialized: bool,
}

impl TuiTerminal {
    /// Create a new TUI terminal manager.
    ///
    /// Call `init()` first, then construct this.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (screen_w, screen_h) = terminal_size();
        let cursor_pos = probe_cursor_position();
        let viewport = Viewport::new(cursor_pos.0, screen_w, screen_h);

        let ratatui_terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

        Ok(Self {
            viewport,
            pending_history: PendingHistory::new(),
            initialized: true,
            ratatui_terminal: Some(ratatui_terminal),
        })
    }

    /// Draw the TUI viewport using the provided draw function.
    ///
    /// Phase 1: simple text rendering (no ratatui).
    /// Phase 2+: uses ratatui `Terminal::draw()` — the render function
    /// receives a `Frame` covering the full terminal, and should render
    /// within the viewport area. The diff mechanism ensures only the
    /// viewport cells are written to the terminal. `draw_with_size()` is
    /// not used because `CrosstermBackend` uses absolute `MoveTo`
    /// commands, which would render at the terminal top rather than the
    /// viewport position.
    pub fn draw(&mut self, draw_fn: impl FnOnce(u16, u16, &mut Vec<String>)) -> Result<(), Box<dyn std::error::Error>> {
        // 1. Flush pending history lines to terminal scrollback
        self.pending_history.flush()?;

        // 2. Update screen dimensions
        let (screen_w, screen_h) = terminal_size();
        self.viewport.handle_resize(screen_w, screen_h);

        // 3. Move cursor to viewport top and clear area
        let top = self.viewport.top();
        let height = self.viewport.height();
        let width = self.viewport.width();

        // 4. Build render buffer
        let mut lines: Vec<String> = Vec::new();
        draw_fn(width, height, &mut lines);

        // 5. Position cursor at viewport top-left
        execute!(
            stdout(),
            crossterm::cursor::MoveTo(0, top),
        )?;

        // 6. Write lines into viewport area
        for (i, line) in lines.iter().enumerate() {
            if i >= height as usize {
                break;
            }
            if i > 0 {
                write!(stdout(), "\r\n")?;
            }
            // Truncate to viewport width
            let display = if line.len() > width as usize {
                format!("{}…", &line[..width.saturating_sub(1) as usize])
            } else {
                format!("{:width$}", line, width = width as usize)
            };
            write!(stdout(), "{}", display)?;
        }

        // 7. Fill remaining lines with blank
        for _i in lines.len()..height as usize {
            write!(stdout(), "\r\n{:width$}", "", width = width as usize)?;
        }

        stdout().flush()?;
        Ok(())
    }

    /// Draw the TUI viewport using ratatui's rendering pipeline.
    ///
    /// Uses inline viewport mode (no alternate screen). The closure receives
    /// a `Frame` whose area covers the full terminal; the renderer should
    /// write within the viewport area `Rect::new(0, top, width, height)`.
    /// Wrapped in `SynchronizedUpdate` to batch all output and avoid flicker.
    ///
    /// `height` sets the desired viewport height (clamped to available space).
    pub fn draw_ratatui(
        &mut self,
        height: u16,
        f: impl FnOnce(&mut Frame),
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 1. Flush pending history lines to terminal scrollback
        self.pending_history.flush()?;

        // 2. Update screen dimensions
        let (screen_w, screen_h) = terminal_size();
        self.viewport.handle_resize(screen_w, screen_h);
        self.viewport.set_height(height);

        // 3. Draw using ratatui pipeline, wrapped in SynchronizedUpdate
        if let Some(terminal) = &mut self.ratatui_terminal {
            execute!(
                stdout(),
                crossterm::terminal::BeginSynchronizedUpdate,
            )?;

            terminal.draw(|frame| {
                f(frame);
            })?;

            execute!(
                stdout(),
                crossterm::terminal::EndSynchronizedUpdate,
            )?;
        }

        stdout().flush()?;
        Ok(())
    }

    /// Insert a history line into the scrollback buffer.
    pub fn insert_history_line(&mut self, line: String, wrap: HistoryLineWrapPolicy) {
        self.pending_history.push(line, wrap);
    }

    /// Access the viewport (for resize handling, etc.)
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    /// Access the viewport mutably
    pub fn viewport_mut(&mut self) -> &mut Viewport {
        &mut self.viewport
    }

    /// Whether the terminal is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}