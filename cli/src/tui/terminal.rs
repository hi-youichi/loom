//! Terminal management for TUI applications.
//!
//! This module provides [`TerminalManager`] which handles terminal state management
//! including raw mode and alternate screen buffer operations.

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};

/// Manages terminal state for TUI applications.
///
/// This struct handles the lifecycle of terminal configuration including:
/// - Raw mode (disabling line buffering and echo)
/// - Alternate screen buffer (separate buffer for TUI)
/// - Automatic cleanup on drop
///
/// # Example
///
/// ```no_run
/// use cli::tui::TerminalManager;
///
/// let mut manager = TerminalManager::new()?;
/// manager.enable_raw_mode();
/// manager.enter_alternate_screen();
///
/// // Use the terminal...
/// let terminal = manager.terminal();
///
/// // Cleanup is automatic when manager goes out of scope
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct TerminalManager {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    raw_mode_enabled: bool,
    alternate_screen: bool,
}

impl TerminalManager {
    /// Creates a new terminal manager with default settings.
    ///
    /// Initializes a terminal with Crossterm backend but does not enable
    /// raw mode or alternate screen by default. Use [`enable_raw_mode`](Self::enable_raw_mode)
    /// and [`enter_alternate_screen`](Self::enter_alternate_screen) to configure these settings.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - stdout is not a valid TTY
    /// - terminal initialization fails
    ///
    /// # Example
    ///
    /// ```no_run
    /// use cli::tui::TerminalManager;
    ///
    /// let manager = TerminalManager::new()?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new() -> io::Result<Self> {
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;

        Ok(Self {
            terminal,
            raw_mode_enabled: false,
            alternate_screen: false,
        })
    }

    /// Enters alternate screen buffer mode.
    ///
    /// This switches to a separate screen buffer, preserving the original
    /// terminal content. The original content is restored when
    /// [`cleanup`](Self::cleanup) is called or when the manager is dropped.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal command fails to execute.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use cli::tui::TerminalManager;
    ///
    /// let mut manager = TerminalManager::new()?;
    /// manager.enter_alternate_screen()?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn enter_alternate_screen(&mut self) -> io::Result<()> {
        if !self.alternate_screen {
            execute!(self.terminal.backend_mut(), EnterAlternateScreen)?;
            self.alternate_screen = true;
        }
        Ok(())
    }

    /// Enables raw mode for direct input handling.
    ///
    /// Raw mode disables line buffering and echo, allowing for immediate
    /// character-by-character input reading. This is essential for
    /// interactive TUI applications.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - stdin is not a valid TTY
    /// - the terminal does not support raw mode
    ///
    /// # Example
    ///
    /// ```no_run
    /// use cli::tui::TerminalManager;
    ///
    /// let mut manager = TerminalManager::new()?;
    /// manager.enable_raw_mode()?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn enable_raw_mode(&mut self) -> io::Result<()> {
        if !self.raw_mode_enabled {
            enable_raw_mode()?;
            self.raw_mode_enabled = true;
        }
        Ok(())
    }

    /// Cleans up terminal state and restores original settings.
    ///
    /// This method:
    /// - Disables raw mode if it was enabled
    /// - Leaves alternate screen if it was entered
    ///
    /// After cleanup, the terminal is restored to its original state.
    /// This method is safe to call multiple times.
    ///
    /// # Errors
    ///
    /// Returns an error if any terminal restoration command fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use cli::tui::TerminalManager;
    ///
    /// let mut manager = TerminalManager::new()?;
    /// manager.enable_raw_mode()?;
    /// manager.enter_alternate_screen()?;
    ///
    /// // Later, restore terminal state
    /// manager.cleanup()?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn cleanup(&mut self) -> io::Result<()> {
        if self.raw_mode_enabled {
            disable_raw_mode()?;
            self.raw_mode_enabled = false;
        }

        if self.alternate_screen {
            execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
            self.alternate_screen = false;
        }

        Ok(())
    }

    /// Returns a mutable reference to the underlying terminal.
    ///
    /// Use this to draw frames and interact with the terminal directly.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use cli::tui::TerminalManager;
    ///
    /// let mut manager = TerminalManager::new()?;
    /// let terminal = manager.terminal();
    ///
    /// terminal.draw(|f| {
    ///     // Draw UI here
    /// })?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn terminal(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }
}

impl Drop for TerminalManager {
    /// Automatically cleans up terminal state when the manager is dropped.
    ///
    /// This ensures the terminal is always restored to its original state,
    /// even if an error occurs or the manager goes out of scope without
    /// explicit cleanup.
    fn drop(&mut self) {
        // Best effort cleanup - ignore errors during drop
        let _ = self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to check if we're running in a TTY environment.
    /// Tests that require TTY will be skipped in CI or non-interactive environments.
    fn is_tty() -> bool {
        // Use crossterm's terminal size check as a proxy for TTY detection
        crossterm::terminal::size().is_ok()
    }

    #[test]
    fn test_terminal_new() {
        // Skip test if not in a TTY environment (e.g., CI)
        if !is_tty() {
            eprintln!("Skipping test_terminal_new: not a TTY environment");
            return;
        }

        let result = TerminalManager::new();
        assert!(
            result.is_ok(),
            "TerminalManager::new() should succeed in TTY environment"
        );

        let manager = result.unwrap();
        assert!(
            !manager.raw_mode_enabled,
            "Raw mode should be disabled initially"
        );
        assert!(
            !manager.alternate_screen,
            "Alternate screen should be disabled initially"
        );
    }

    #[test]
    fn test_raw_mode_enable_disable() {
        // Skip test if not in a TTY environment (e.g., CI)
        if !is_tty() {
            eprintln!("Skipping test_raw_mode_enable_disable: not a TTY environment");
            return;
        }

        let mut manager = TerminalManager::new().expect("Failed to create TerminalManager");

        // Enable raw mode
        let enable_result = manager.enable_raw_mode();
        assert!(
            enable_result.is_ok(),
            "enable_raw_mode() should succeed in TTY environment"
        );
        assert!(
            manager.raw_mode_enabled,
            "raw_mode_enabled should be true after enable_raw_mode()"
        );

        // Cleanup should disable raw mode
        let cleanup_result = manager.cleanup();
        assert!(
            cleanup_result.is_ok(),
            "cleanup() should succeed"
        );
        assert!(
            !manager.raw_mode_enabled,
            "raw_mode_enabled should be false after cleanup()"
        );
    }

    #[test]
    fn test_cleanup() {
        // Skip test if not in a TTY environment (e.g., CI)
        if !is_tty() {
            eprintln!("Skipping test_cleanup: not a TTY environment");
            return;
        }

        let mut manager = TerminalManager::new().expect("Failed to create TerminalManager");

        // Enable both features
        let _ = manager.enable_raw_mode();
        let _ = manager.enter_alternate_screen();

        // Cleanup should restore everything
        let cleanup_result = manager.cleanup();
        assert!(
            cleanup_result.is_ok(),
            "cleanup() should succeed"
        );
        assert!(
            !manager.raw_mode_enabled,
            "raw_mode_enabled should be false after cleanup()"
        );
        assert!(
            !manager.alternate_screen,
            "alternate_screen should be false after cleanup()"
        );

        // Cleanup should be idempotent
        let second_cleanup = manager.cleanup();
        assert!(
            second_cleanup.is_ok(),
            "Second cleanup() should also succeed"
        );
    }

    #[test]
    fn test_terminal_reference() {
        // Skip test if not in a TTY environment (e.g., CI)
        if !is_tty() {
            eprintln!("Skipping test_terminal_reference: not a TTY environment");
            return;
        }

        let mut manager = TerminalManager::new().expect("Failed to create TerminalManager");
        let terminal = manager.terminal();

        // Just verify we can get a reference
        let _ = terminal.size();
    }
}
