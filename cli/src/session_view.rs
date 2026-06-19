//! Session list presentation layer.
//!
//! Decoupled from `SessionManager` (data layer) and `ListArgs` (CLI layer).
//! Accepts a `SessionListDisplayConfig` struct that can be built from CLI args
//! or config file defaults.

use chrono::{DateTime, Utc};
use loom_stream_display::terminal as term;

use crate::session::SessionInfo;

// ── Constants ───────────────────────────────────────────────────────

/// Width (in terminal columns) of the SESSION ID column in the table view.
const SESSION_ID_COL_WIDTH: usize = 44;

/// Number of chars kept for the `%h` template placeholder.
const SESSION_ID_TEMPLATE_CHARS: usize = 8;

// ── Display config (P2: decouple from ListArgs) ─────────────────────

/// Configuration for session list rendering.
///
/// Built by the caller from CLI args + config defaults. The presentation
/// layer never touches clap structures directly.
#[derive(Debug, Clone, Default)]
pub struct SessionListDisplayConfig {
    /// Use one-line compact format (like `git log --oneline`).
    pub oneline: bool,
    /// Custom template with placeholders (%h %i %t %c %s %r %d %%).
    pub format: Option<String>,
}

// ── Public API ──────────────────────────────────────────────────────

/// Formats session list as a string (no I/O).
///
/// Dispatch order: `format` template > `oneline` > default table.
pub fn format_session_list(sessions: &[SessionInfo], cfg: &SessionListDisplayConfig) -> String {
    if sessions.is_empty() {
        return "No sessions found.\n".to_string();
    }
    if let Some(template) = &cfg.format {
        render_template(template, sessions)
    } else if cfg.oneline {
        build_oneline(sessions)
    } else {
        build_table(sessions)
    }
}

/// Prints session list as JSON.
pub fn print_json(sessions: &[SessionInfo]) -> Result<(), String> {
    let json_output = serde_json::to_string_pretty(sessions)
        .map_err(|e| format!("Failed to serialize to JSON: {}", e))?;
    println!("{}", json_output);
    Ok(())
}

// ── Format builders ─────────────────────────────────────────────────

/// Compact one-line format: `<id>  <relative-time>  <title> (<N>)`.
fn build_oneline(sessions: &[SessionInfo]) -> String {
    let now = Utc::now();
    let total_width = term::get_terminal_width();
    let reserved = SESSION_ID_COL_WIDTH + 6 + 20 + 8;
    let title_budget = total_width.saturating_sub(reserved).max(10);

    let mut out = String::new();
    for s in sessions {
        let id = truncate_session_id_plain(&s.session_id, SESSION_ID_COL_WIDTH);
        let rel = format_relative_time(s.last_updated.as_ref(), now);
        let title_raw = s.title.as_deref().unwrap_or("(untitled)");
        let title = truncate_to_display_width(title_raw, title_budget);

        let id_colored = paint(&id, AnsiStyle::Cyan);
        let rel_colored = paint(&rel, AnsiStyle::Dim);
        out.push_str(&format!(
            "{}  {}  {} ({})\n",
            id_colored, rel_colored, title, s.checkpoint_count
        ));
    }
    out.push('\n');
    out.push_str(&format!("Total sessions: {}\n", sessions.len()));
    out
}

/// Terminal-width-adaptive table format.
fn build_table(sessions: &[SessionInfo]) -> String {
    let total_width = term::get_terminal_width();
    let fixed_cols = SESSION_ID_COL_WIDTH + 12 + 6 + 8 + 4;
    let title_width = total_width.saturating_sub(fixed_cols + 2).max(10);

    let now = Utc::now();
    let mut out = String::new();
    out.push_str(&format!(
        "{}  {}  {}  {}  {}\n",
        paint(&pad_to_display_width("SESSION ID", SESSION_ID_COL_WIDTH), AnsiStyle::Dim),
        paint(&pad_to_display_width("LAST UPDATED", 12), AnsiStyle::Dim),
        paint(&pad_to_display_width("STEPS", 6), AnsiStyle::Dim),
        paint(&pad_to_display_width("LATEST", 8), AnsiStyle::Dim),
        paint(&truncate_to_display_width("TITLE", title_width), AnsiStyle::Dim),
    ));
    out.push_str(&"-".repeat(total_width));
    out.push('\n');

    for s in sessions {
        let short_id = truncate_session_id_plain(&s.session_id, SESSION_ID_COL_WIDTH);
        let rel = format_relative_time(s.last_updated.as_ref(), now);
        out.push_str(&format!(
            "{}  {}  {}  {}  {}\n",
            paint(&pad_to_display_width(&short_id, SESSION_ID_COL_WIDTH), AnsiStyle::Cyan),
            paint(&pad_to_display_width(&rel, 12), AnsiStyle::Dim),
            pad_to_display_width(&s.checkpoint_count.to_string(), 6),
            pad_to_display_width(&s.latest_source, 8),
            truncate_to_display_width(
                s.title.as_deref().unwrap_or("(untitled)"),
                title_width,
            ),
        ));
    }

    out.push('\n');
    out.push_str(&format!("Total sessions: {}\n", sessions.len()));
    out
}

/// Renders a custom template against each session.
fn render_template(template: &str, sessions: &[SessionInfo]) -> String {
    let now = Utc::now();
    let mut out = String::new();
    for s in sessions {
        out.push_str(&render_format_string(template, s, now));
        out.push('\n');
    }
    out
}

// ── Template engine ─────────────────────────────────────────────────

/// Expands placeholders in `template` against `session`, returning one line.
fn render_format_string(
    template: &str,
    session: &SessionInfo,
    now: DateTime<Utc>,
) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let Some(&next) = chars.peek() else {
            out.push('%');
            break;
        };
        chars.next();
        match next {
            'h' => out.push_str(&shorten_session_id(&session.session_id, SESSION_ID_TEMPLATE_CHARS)),
            'i' => out.push_str(&session.session_id),
            't' => out.push_str(session.title.as_deref().unwrap_or("(untitled)")),
            'c' => out.push_str(&session.checkpoint_count.to_string()),
            's' => out.push_str(&session.latest_source),
            'r' => out.push_str(&format_relative_time(session.last_updated.as_ref(), now)),
            'd' => out.push_str(&format_absolute_time(session.last_updated.as_ref())),
            '%' => out.push('%'),
            other => {
                out.push('%');
                out.push(other);
            }
        }
    }
    out
}

// ── String helpers ──────────────────────────────────────────────────

/// Truncates a session id to `take_chars` chars without an ellipsis.
fn truncate_session_id_plain(id: &str, take_chars: usize) -> String {
    if id.chars().count() <= take_chars {
        id.to_string()
    } else {
        id.chars().take(take_chars).collect()
    }
}

/// Shortens a session id to its first `take_chars` chars + `…`.
fn shorten_session_id(id: &str, take_chars: usize) -> String {
    if id.chars().count() <= take_chars {
        id.to_string()
    } else {
        let mut out: String = id.chars().take(take_chars).collect();
        out.push('…');
        out
    }
}

/// Truncates a string by **display width** (handling CJK double-width chars).
fn truncate_to_display_width(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if UnicodeWidthStr::width(s) <= max_width {
        return s.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let budget = max_width.saturating_sub(1);
    let mut width = 0;
    let mut result = String::new();
    for ch in s.chars() {
        let ch_w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_w > budget {
            result.push('…');
            return result;
        }
        result.push(ch);
        width += ch_w;
    }
    result
}

/// Left-pads a string to at least `min_width` display columns.
fn pad_to_display_width(s: &str, min_width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    let w = UnicodeWidthStr::width(s);
    if w >= min_width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(min_width - w))
    }
}

/// Formats a `DateTime<Utc>` as relative time using `chrono-humanize`.
fn format_relative_time(dt: Option<&DateTime<Utc>>, now: DateTime<Utc>) -> String {
    use chrono_humanize::{Accuracy, HumanTime, Tense};
    let Some(dt) = dt else {
        return "N/A".to_string();
    };
    let delta = now.signed_duration_since(*dt);
    HumanTime::from(delta).to_text_en(Accuracy::Rough, Tense::Past)
}

/// Formats a `DateTime<Utc>` as absolute UTC date.
fn format_absolute_time(dt: Option<&DateTime<Utc>>) -> String {
    dt.map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "N/A".to_string())
}

// ── P5: ANSI styling unified via panel_format ───────────────────────

/// Output color for session-list rendering.
#[derive(Debug, Clone, Copy)]
enum AnsiStyle {
    Dim,
    Cyan,
}

/// Wraps `text` in the requested ANSI style, only when stdout is a TTY.
///
/// Callers must pass an already-padded/truncated string — ANSI escape codes
/// are zero-width, so wrapping after width computation keeps alignment correct.
fn paint(text: &str, style: AnsiStyle) -> String {
    if !term::stdout_color_enabled() {
        return text.to_string();
    }
    match style {
        AnsiStyle::Dim => format!("\x1b[2m{}\x1b[0m", text),
        AnsiStyle::Cyan => format!("\x1b[36m{}\x1b[0m", text),
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_session() -> SessionInfo {
        SessionInfo {
            session_id: "abcdef1234567890".to_string(),
            checkpoint_count: 42,
            created_at: Some(DateTime::from_timestamp_millis(1_700_000_000_000).unwrap()),
            last_updated: Some(DateTime::from_timestamp_millis(1_725_000_000_000).unwrap()),
            latest_step: 7,
            latest_source: "Loop".to_string(),
            title: Some("审计 dsgil 优化层".to_string()),
        }
    }

    // ── render_format_string ───────────────────────────────────

    #[test]
    fn render_format_percent_h_short_id() {
        let s = fixture_session();
        let now = DateTime::from_timestamp_millis(1_725_000_000_000).unwrap();
        assert_eq!(render_format_string("%h", &s, now), "abcdef12…");
    }

    #[test]
    fn render_format_percent_i_full_id() {
        let s = fixture_session();
        let now = DateTime::from_timestamp_millis(1_725_000_000_000).unwrap();
        assert_eq!(render_format_string("%i", &s, now), "abcdef1234567890");
    }

    #[test]
    fn render_format_percent_t_title() {
        let s = fixture_session();
        let now = DateTime::from_timestamp_millis(1_725_000_000_000).unwrap();
        assert_eq!(render_format_string("%t", &s, now), "审计 dsgil 优化层");
    }

    #[test]
    fn render_format_percent_t_untitled() {
        let mut s = fixture_session();
        s.title = None;
        let now = DateTime::from_timestamp_millis(1_725_000_000_000).unwrap();
        assert_eq!(render_format_string("%t", &s, now), "(untitled)");
    }

    #[test]
    fn render_format_percent_c_count() {
        let s = fixture_session();
        let now = DateTime::from_timestamp_millis(1_725_000_000_000).unwrap();
        assert_eq!(render_format_string("%c", &s, now), "42");
    }

    #[test]
    fn render_format_percent_s_source() {
        let s = fixture_session();
        let now = DateTime::from_timestamp_millis(1_725_000_000_000).unwrap();
        assert_eq!(render_format_string("%s", &s, now), "Loop");
    }

    #[test]
    fn render_format_percent_d_absolute_date() {
        let s = fixture_session();
        let now = DateTime::from_timestamp_millis(1_725_000_000_000).unwrap();
        let out = render_format_string("%d", &s, now);
        assert!(out.contains("2024"));
        assert!(out.ends_with("UTC"));
    }

    #[test]
    fn render_format_percent_r_relative_past() {
        let s = fixture_session();
        let now = s.last_updated.unwrap() + chrono::Duration::seconds(3600);
        let out = render_format_string("%r", &s, now);
        assert!(out.contains("ago"), "expected 'ago' suffix, got: {}", out);
    }

    #[test]
    fn render_format_percent_percent_literal() {
        let s = fixture_session();
        let now = DateTime::from_timestamp_millis(1_725_000_000_000).unwrap();
        assert_eq!(render_format_string("100%%", &s, now), "100%");
    }

    #[test]
    fn render_format_unknown_placeholder_emitted_literally() {
        let s = fixture_session();
        let now = DateTime::from_timestamp_millis(1_725_000_000_000).unwrap();
        assert_eq!(render_format_string("%z", &s, now), "%z");
        assert_eq!(
            render_format_string("%h %x %i", &s, now),
            "abcdef12… %x abcdef1234567890"
        );
    }

    #[test]
    fn render_format_trailing_percent() {
        let s = fixture_session();
        let now = DateTime::from_timestamp_millis(1_725_000_000_000).unwrap();
        assert_eq!(render_format_string("abc%", &s, now), "abc%");
    }

    #[test]
    fn render_format_combined_template() {
        let s = fixture_session();
        let now = s.last_updated.unwrap() + chrono::Duration::seconds(60);
        let out = render_format_string("[%h] %t (%c)", &s, now);
        assert_eq!(out, "[abcdef12…] 审计 dsgil 优化层 (42)");
    }

    #[test]
    fn render_format_plain_text_passes_through() {
        let s = fixture_session();
        let now = DateTime::from_timestamp_millis(1_725_000_000_000).unwrap();
        assert_eq!(render_format_string("hello world", &s, now), "hello world");
    }

    #[test]
    fn render_format_empty_template() {
        let s = fixture_session();
        let now = DateTime::from_timestamp_millis(1_725_000_000_000).unwrap();
        assert_eq!(render_format_string("", &s, now), "");
    }

    // ── shorten_session_id ─────────────────────────────────────

    #[test]
    fn shorten_id_take_8() {
        assert_eq!(shorten_session_id("abcdef1234567890", 8), "abcdef12…");
    }

    #[test]
    fn shorten_id_shorter_returns_full() {
        assert_eq!(shorten_session_id("abc", 8), "abc");
    }

    #[test]
    fn shorten_id_unicode_safe() {
        assert_eq!(shorten_session_id("会话abc123", 4), "会话ab…");
    }

    // ── truncate_session_id_plain ──────────────────────────────

    #[test]
    fn truncate_plain_id_full_uuid_fits() {
        let uuid = "session-00720cdb-151c-497a-a4f5-374a37aa130b";
        assert_eq!(truncate_session_id_plain(uuid, 44), uuid);
    }

    #[test]
    fn truncate_plain_id_shorter_returns_full() {
        assert_eq!(
            truncate_session_id_plain("session-1781862313477", 44),
            "session-1781862313477"
        );
    }

    #[test]
    fn truncate_plain_id_long_truncated_silently() {
        let long = "sub-session-00720cdb-151c-497a-a4f5-374a37aa130b-explore-0";
        let truncated = truncate_session_id_plain(long, 44);
        assert_eq!(truncated.len(), 44);
        assert!(!truncated.ends_with('…'));
    }

    // ── truncate_to_display_width ──────────────────────────────

    #[test]
    fn truncate_to_display_width_short_unchanged() {
        assert_eq!(truncate_to_display_width("hello", 10), "hello");
    }

    #[test]
    fn truncate_to_display_width_long_appends_ellipsis() {
        let out = truncate_to_display_width("hello world this is a long title", 10);
        assert!(out.ends_with('…'));
        assert_eq!(unicode_width::UnicodeWidthStr::width(out.as_str()), 10);
    }

    #[test]
    fn truncate_to_display_width_cjk_aware() {
        let out = truncate_to_display_width("你好世界hello world", 10);
        assert_eq!(unicode_width::UnicodeWidthStr::width(out.as_str()), 10);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn truncate_to_display_width_zero_returns_empty() {
        assert_eq!(truncate_to_display_width("hello", 0), "");
    }
}
