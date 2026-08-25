//! Terminal capability detection and ANSI styling utilities.
//!
//! Generic utilities with no anureo-specific concepts. Any Rust CLI crate
//! can depend on this module.

// ── Terminal size / TTY detection ───────────────────────────────────

/// Returns the current terminal width in columns, defaulting to 80 on failure.
pub fn get_terminal_width() -> usize {
    termsize::get().map(|s| s.cols as usize).unwrap_or(80)
}

/// Returns true when stdout is connected to a TTY.
pub fn is_stdout_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

/// Returns true when stderr is connected to a TTY.
pub fn is_stderr_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

// ── Color enablement ────────────────────────────────────────────────

/// Returns true when stderr is connected to a TTY and `NO_COLOR` is unset.
///
/// Use this for color decisions on **stderr** output (e.g. panel formatting,
/// spinners, progress indicators).
pub fn stderr_color_enabled() -> bool {
    is_stderr_tty() && std::env::var("NO_COLOR").is_err()
}

/// Returns true when stdout is connected to a TTY and `NO_COLOR` is unset.
///
/// Use this for color decisions on **stdout** output (e.g. CLI tables,
/// `session list` formatting).
pub fn stdout_color_enabled() -> bool {
    is_stdout_tty() && std::env::var("NO_COLOR").is_err()
}

// ── ANSI wrappers (stream-aware) ────────────────────────────────────

/// Wraps text in a dim ANSI style (for thinking content, secondary info).
///
/// Checks **stderr** color enablement — use `dim_stdout` for stdout output.
pub fn dim(text: &str) -> String {
    if stderr_color_enabled() {
        format!("\x1b[2m{}\x1b[0m", text)
    } else {
        text.to_string()
    }
}

/// Wraps text in a dim ANSI style, gated on **stdout** color enablement.
pub fn dim_stdout(text: &str) -> String {
    if stdout_color_enabled() {
        format!("\x1b[2m{}\x1b[0m", text)
    } else {
        text.to_string()
    }
}

/// Wraps text in a green ANSI style (for success/completion).
///
/// Checks **stderr** color enablement.
pub fn green(text: &str) -> String {
    if stderr_color_enabled() {
        format!("\x1b[32m{}\x1b[0m", text)
    } else {
        text.to_string()
    }
}

/// Wraps text in a yellow ANSI style (for tool calls).
///
/// Checks **stderr** color enablement.
pub fn yellow(text: &str) -> String {
    if stderr_color_enabled() {
        format!("\x1b[33m{}\x1b[0m", text)
    } else {
        text.to_string()
    }
}

/// Wraps text in a bold style.
///
/// Checks **stderr** color enablement.
pub fn bold(text: &str) -> String {
    if stderr_color_enabled() {
        format!("\x1b[1m{}\x1b[0m", text)
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_terminal_width_returns_at_least_1() {
        // In test env (piped), falls back to 80.
        assert!(get_terminal_width() >= 80);
    }

    #[test]
    fn dim_returns_text_regardless_of_tty() {
        let result = dim("hello");
        assert!(result.contains("hello"));
    }

    #[test]
    fn dim_stdout_returns_text_regardless_of_tty() {
        let result = dim_stdout("hello");
        assert!(result.contains("hello"));
    }
}
