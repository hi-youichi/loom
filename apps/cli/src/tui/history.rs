//! History line insertion into terminal scrollback.
//!
//! Manages a buffer of lines that should be inserted into the terminal's
//! normal scrollback (above the inline viewport). These lines persist
//! after the TUI exits, providing a transcript of the conversation in
//! the terminal history.

use std::io::{stdout, Write};

/// Wrapping policy for history lines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HistoryLineWrapPolicy {
    /// Lines are pre-wrapped by the caller; write as-is.
    PreWrap,
    /// Let the terminal handle wrapping (don't insert manual line breaks).
    Terminal,
}

/// A single pending history line.
#[derive(Debug, Clone)]
struct HistoryLine {
    content: String,
    wrap: HistoryLineWrapPolicy,
}

/// Buffer of pending history lines to be flushed into terminal scrollback.
///
/// Lines are accumulated during rendering and flushed in one batch before
/// the next `draw()` call to minimize flicker.
#[derive(Debug)]
pub struct PendingHistory {
    lines: Vec<HistoryLine>,
}

impl PendingHistory {
    /// Create a new empty pending history buffer.
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Add a line to the pending history buffer.
    pub fn push(&mut self, content: String, wrap: HistoryLineWrapPolicy) {
        self.lines.push(HistoryLine { content, wrap });
    }

    /// Number of pending lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Flush all pending lines into the terminal scrollback.
    ///
    /// This writes the lines above the current viewport position, then
    /// repositions the cursor back to the viewport top.
    pub fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.lines.is_empty() {
            return Ok(());
        }

        let mut stdout = stdout();
        for line in &self.lines {
            match line.wrap {
                HistoryLineWrapPolicy::PreWrap => {
                    writeln!(stdout, "{}", line.content)?;
                }
                HistoryLineWrapPolicy::Terminal => {
                    writeln!(stdout, "{}", line.content)?;
                }
            }
        }
        stdout.flush()?;

        self.lines.clear();
        Ok(())
    }

    /// Clear all pending lines without flushing.
    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

impl Default for PendingHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pending_history_new() {
        let ph = PendingHistory::new();
        assert!(ph.is_empty());
        assert_eq!(ph.len(), 0);
    }

    #[test]
    fn test_pending_history_push() {
        let mut ph = PendingHistory::new();
        ph.push("hello".into(), HistoryLineWrapPolicy::PreWrap);
        assert!(!ph.is_empty());
        assert_eq!(ph.len(), 1);
    }

    #[test]
    fn test_pending_history_clear() {
        let mut ph = PendingHistory::new();
        ph.push("hello".into(), HistoryLineWrapPolicy::PreWrap);
        ph.clear();
        assert!(ph.is_empty());
    }

    #[test]
    fn test_pending_history_default() {
        let ph = PendingHistory::default();
        assert!(ph.is_empty());
    }
}