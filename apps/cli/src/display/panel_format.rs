//! Structured panel format for stderr output.
//!
//! Shared by both CLI and goal runner for unified UX.
//! Provides unified formatting for:
//! - Panel lines: `_CATEGORY  message` (with ANSI color)
//! - Tool call status: `tool_name: args` / `tool_name: args ✓ result (X.Xs)`
//! - LLM usage: `_USAGE  2.35s | 1.2K↑ 800↓ = 2.0K @ 850 t/s`
//! - Thinking separator

use std::time::Duration;

// ANSI color wrappers come from `terminal` module.
use super::terminal::{bold, green, yellow};

/// Backward-compat alias: checks stderr TTY + NO_COLOR.
fn color_enabled() -> bool {
    super::terminal::stderr_color_enabled()
}

// ── Panel line formatting ────────────────────────────────────────────

/// Formats a panel line: `_CATEGORY  message`.
pub fn format_panel_line(category: &str, message: &str) -> String {
    let padded = format!("{:<8}", category.to_uppercase());
    if color_enabled() {
        format!("{}  {}", bold(&padded), message)
    } else {
        format!("_{}  {}", padded.trim_end(), message)
    }
}

// ── Tool call formatting ────────────────────────────────────────────

fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> &str {
    if max_bytes >= s.len() {
        return s;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &s[..boundary]
}

pub fn format_tool_call(tool_name: &str, args_summary: &str) -> String {
    let args = if args_summary.len() > 80 {
        format!("{}…", truncate_to_char_boundary(args_summary, 77))
    } else {
        args_summary.to_string()
    };
    format!("{}: {}", yellow(tool_name), args)
}

pub fn format_tool_done(tool_name: &str, result_summary: &str, elapsed: Option<Duration>) -> String {
    let timing = match elapsed {
        Some(d) => format!(" {}", super::tool_summary::format_elapsed(d)),
        None => String::new(),
    };
    let summary = if result_summary.is_empty() {
        String::new()
    } else if result_summary.len() > 60 {
        format!(" — {}", &truncate_to_char_boundary(result_summary, 57))
    } else {
        format!(" — {}", result_summary)
    };
    format!("{}{}{} {}", yellow(tool_name), summary, timing, green("✓"))
}

// ── LLM Usage formatting ───────────────────────────────────────────

pub fn format_tokens(t: u32) -> String {
    if t >= 1_000_000 {
        format!("{:.1}M", t as f64 / 1_000_000.0)
    } else if t >= 1000 {
        format!("{:.1}K", t as f64 / 1000.0)
    } else {
        t.to_string()
    }
}

pub fn format_usage_line(
    duration: Duration,
    prompt_tokens: u32,
    completion_tokens: u32,
    prefill_duration: Option<Duration>,
    decode_duration: Option<Duration>,
    verbose: bool,
) -> String {
    let secs = duration.as_secs_f64();
    let total = prompt_tokens as u64 + completion_tokens as u64;
    let tps = if secs > 0.0 { total as f64 / secs } else { 0.0 };

    if verbose {
        if let (Some(pf), Some(dc)) = (prefill_duration, decode_duration) {
            let pf_secs = pf.as_secs_f64();
            let dc_secs = dc.as_secs_f64();
            let pf_rate = if pf_secs > 0.0 { prompt_tokens as f64 / pf_secs } else { 0.0 };
            let dc_rate = if dc_secs > 0.0 { completion_tokens as f64 / dc_secs } else { 0.0 };
            format_panel_line(
                "USAGE",
                &format!(
                    "{:.2}s | prefill: {}/{:.2}s={:.0} t/s | decode: {}/{:.2}s={:.0} t/s | total: {} @ {:.0} t/s",
                    secs, format_tokens(prompt_tokens), pf_secs, pf_rate,
                    format_tokens(completion_tokens), dc_secs, dc_rate,
                    format_tokens(prompt_tokens + completion_tokens), tps,
                ),
            )
        } else {
            format_panel_line(
                "USAGE",
                &format!(
                    "{:.2}s | {}↑ {}↓ = {} @ {:.0} t/s",
                    secs, format_tokens(prompt_tokens), format_tokens(completion_tokens),
                    format_tokens(prompt_tokens + completion_tokens), tps,
                ),
            )
        }
    } else {
        format_panel_line(
            "USAGE",
            &format!(
                "{:.2}s | {}↑ {}↓ @ {:.0} t/s",
                secs, format_tokens(prompt_tokens), format_tokens(completion_tokens), tps,
            ),
        )
    }
}

// ── Banner formatting ───────────────────────────────────────────────

pub fn format_agent_line(name: &str, source: &str, description: Option<&str>) -> String {
    let desc = description.map(|d| format!(" — {}", d)).unwrap_or_default();
    format_panel_line("AGENT", &format!("{} ({}){}", name, source, desc))
}

pub fn format_tools_line(tool_names: &[&str]) -> String {
    format_panel_line("TOOLS", &tool_names.join(", "))
}

pub fn format_model_line(model_name: &str, context_info: &str) -> String {
    format_panel_line("MODEL", &format!("{} ({})", model_name, context_info))
}

pub fn format_thinking_separator() -> String {
    if color_enabled() {
        "\x1b[2m────────────────────\x1b[0m".to_string()
    } else {
        "────────────────────".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_panel_line_produces_category_prefix() {
        let line = format_panel_line("AGENT", "dev (project) — Code assistant");
        assert!(line.contains("AGENT"));
        assert!(line.contains("dev (project)"));
    }

    #[test]
    fn format_tool_call_contains_tool_name() {
        let line = format_tool_call("bash", "echo hello");
        assert!(line.contains("bash"));
        assert!(line.contains("echo hello"));
        assert!(!line.contains("CALL"));
    }

    #[test]
    fn format_tool_done_contains_checkmark() {
        let line = format_tool_done("read", "3 lines", None);
        assert!(line.contains("✓"));
        assert!(!line.contains("DONE"));
    }

    #[test]
    fn format_tool_call_truncates_long_args() {
        let long_args: String = "x".repeat(100);
        let line = format_tool_call("bash", &long_args);
        assert!(line.contains("…"));
        assert!(line.len() < 120);
    }

    #[test]
    fn format_usage_line_normal_mode() {
        let line = format_usage_line(Duration::from_secs_f64(2.35), 1200, 800, None, None, false);
        assert!(line.contains("2.35s"));
        assert!(line.contains("t/s"));
    }

    #[test]
    fn format_usage_line_verbose_with_prefill_decode() {
        let line = format_usage_line(
            Duration::from_secs_f64(2.35), 1200, 800,
            Some(Duration::from_secs_f64(0.85)), Some(Duration::from_secs_f64(1.50)),
            true,
        );
        assert!(line.contains("prefill:"));
        assert!(line.contains("decode:"));
    }

    #[test]
    fn format_usage_line_verbose_without_prefill_decode() {
        let line = format_usage_line(Duration::from_secs_f64(2.35), 1200, 800, None, None, true);
        assert!(line.contains("2.35s"));
        assert!(line.contains("↓"));
        assert!(line.contains("↑"));
    }

    #[test]
    fn format_tokens_human_readable() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1200), "1.2K");
        assert!(format_tokens(1_500_000).contains("M"));
    }

    #[test]
    fn format_thinking_separator_produces_line() {
        let sep = format_thinking_separator();
        assert!(sep.contains("────"));
    }
}
