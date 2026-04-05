//! Unified tool output normalization to control context size.
//!
//! This module provides a unified normalization layer that transforms raw tool outputs
//! into structured results before they enter the ReAct state. This prevents large
//! tool outputs from exploding the LLM context in subsequent turns.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Strategy for normalizing tool output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutputStrategy {
    /// Small result, keep inline in full.
    Inline,
    /// Only keep a summary, no inline content.
    SummaryOnly,
    /// Keep head and tail excerpts, suitable for logs/commands.
    HeadTail,
    /// Persist to file, return only file reference.
    FileRef,
    /// Persist to file with a small excerpt.
    FileRefWithExcerpt,
}

impl Default for ToolOutputStrategy {
    fn default() -> Self {
        Self::Inline
    }
}

/// Optional metadata supplied by a tool to influence output normalization.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolOutputHint {
    /// Strong preference for a specific normalization strategy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_strategy: Option<ToolOutputStrategy>,
    /// Safe inline budget for this tool when the default inline limit is too high.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_inline_chars: Option<usize>,
    /// Whether this tool generally benefits more from head/tail excerpts than summaries.
    #[serde(default)]
    pub prefer_head_tail: bool,
}

/// Reference to persisted tool output storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStorageRef {
    /// File path where the output is stored.
    pub path: PathBuf,
    /// Size in bytes.
    pub size: usize,
    /// Content type (e.g., "text/plain", "application/json").
    pub content_type: String,
    /// Encoding (e.g., "utf-8").
    pub encoding: String,
    /// Tool name that produced this output.
    pub tool_name: String,
}

/// Configuration for tool output normalization.
#[derive(Debug, Clone)]
pub struct NormalizationConfig {
    /// Maximum characters for inline results (default: 4000).
    pub inline_limit: usize,
    /// Maximum characters for display text (default: 1000).
    pub display_limit: usize,
    /// Maximum characters for excerpts (default: 1200).
    pub excerpt_limit: usize,
    /// Maximum characters for head/tail portions (default: 600 each).
    pub head_tail_limit: usize,
    /// Threshold to switch to file-based storage (default: 10000).
    pub file_ref_threshold: usize,
    /// Maximum observation chars for a single turn (default: 8000).
    pub observation_budget: usize,
    /// Observation chars already consumed by previous tool outputs in this turn.
    pub used_observation_chars: usize,
    /// Whether to persist large outputs to disk.
    pub enable_persistence: bool,
    /// Base directory for persisted outputs.
    pub storage_dir: Option<PathBuf>,
}

impl Default for NormalizationConfig {
    fn default() -> Self {
        Self {
            inline_limit: 4_000,
            display_limit: 1_000,
            excerpt_limit: 1_200,
            head_tail_limit: 600,
            file_ref_threshold: 10_000,
            observation_budget: 8_000,
            used_observation_chars: 0,
            enable_persistence: false,
            storage_dir: None,
        }
    }
}

impl NormalizationConfig {
    /// Runtime defaults used by the ReAct pipeline.
    pub fn runtime_default() -> Self {
        Self {
            enable_persistence: true,
            storage_dir: Some(default_tool_output_dir()),
            ..Self::default()
        }
    }

    /// Returns a copy updated with observation chars already used in this turn.
    pub fn with_used_observation_chars(mut self, used_observation_chars: usize) -> Self {
        self.used_observation_chars = used_observation_chars;
        self
    }

    fn remaining_observation_budget(&self) -> usize {
        self.observation_budget
            .saturating_sub(self.used_observation_chars)
    }
}

/// Normalized tool output result.
#[derive(Debug, Clone)]
pub struct NormalizedToolOutput {
    /// Original raw content (may be None for large results).
    pub raw_content: Option<String>,
    /// Text to inject into next LLM turn (ObserveNode uses this).
    pub observation_text: String,
    /// Text to show in stream/UI events.
    pub display_text: String,
    /// Reference to persisted storage (if applicable).
    pub storage_ref: Option<ToolStorageRef>,
    /// Strategy applied to this output.
    pub strategy: ToolOutputStrategy,
    /// Character count of original raw output.
    pub raw_chars: usize,
    /// Character count of observation text.
    pub observation_chars: usize,
    /// Whether the output was truncated.
    pub truncated: bool,
}

impl Default for NormalizedToolOutput {
    fn default() -> Self {
        Self {
            raw_content: None,
            observation_text: String::new(),
            display_text: String::new(),
            storage_ref: None,
            strategy: ToolOutputStrategy::Inline,
            raw_chars: 0,
            observation_chars: 0,
            truncated: false,
        }
    }
}

static TOOL_OUTPUT_FILE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Normalize tool output using the unified strategy.
pub fn normalize_tool_output(
    tool_name: &str,
    args: &serde_json::Value,
    raw_text: &str,
    is_error: bool,
    output_hint: Option<&ToolOutputHint>,
    config: NormalizationConfig,
) -> NormalizedToolOutput {
    let raw_chars = raw_text.chars().count();
    let remaining_budget = config.remaining_observation_budget();
    let strategy = determine_strategy(
        tool_name,
        raw_chars,
        is_error,
        output_hint,
        remaining_budget,
        &config,
    );
    let storage_ref = if matches!(
        strategy,
        ToolOutputStrategy::FileRef | ToolOutputStrategy::FileRefWithExcerpt
    ) {
        persist_output(tool_name, args, raw_text, &config)
    } else {
        None
    };

    let mut output = match strategy {
        ToolOutputStrategy::Inline => build_inline_output(raw_text, raw_chars, &config),
        ToolOutputStrategy::HeadTail => build_head_tail_output(raw_text, raw_chars, &config),
        ToolOutputStrategy::SummaryOnly => {
            build_summary_output(tool_name, args, raw_text, is_error, raw_chars, storage_ref, &config)
        }
        ToolOutputStrategy::FileRef | ToolOutputStrategy::FileRefWithExcerpt => build_file_ref_output(
            tool_name,
            raw_text,
            raw_chars,
            strategy,
            storage_ref,
            remaining_budget,
            &config,
        ),
    };

    apply_observation_budget(&mut output, remaining_budget);
    output
}

fn build_inline_output(
    raw_text: &str,
    raw_chars: usize,
    config: &NormalizationConfig,
) -> NormalizedToolOutput {
    NormalizedToolOutput {
        raw_content: Some(raw_text.to_string()),
        observation_text: raw_text.to_string(),
        display_text: truncate_text(raw_text, config.display_limit),
        storage_ref: None,
        strategy: ToolOutputStrategy::Inline,
        raw_chars,
        observation_chars: raw_chars,
        truncated: false,
    }
}

fn build_head_tail_output(
    raw_text: &str,
    raw_chars: usize,
    config: &NormalizationConfig,
) -> NormalizedToolOutput {
    let (head, tail) = split_head_tail(raw_text, config.head_tail_limit);
    let observation_text = if tail.is_empty() {
        head.clone()
    } else {
        format!("Head:\n{}\n...\nTail:\n{}", head, tail)
    };
    let display_text = if tail.is_empty() {
        truncate_text(&head, config.display_limit)
    } else {
        format!(
            "{}...{}",
            truncate_text(&head, config.display_limit / 2),
            truncate_text(&tail, config.display_limit / 2)
        )
    };

    NormalizedToolOutput {
        raw_content: if raw_chars > config.file_ref_threshold {
            None
        } else {
            Some(raw_text.to_string())
        },
        observation_text: observation_text.clone(),
        display_text,
        storage_ref: None,
        strategy: ToolOutputStrategy::HeadTail,
        raw_chars,
        observation_chars: observation_text.chars().count(),
        truncated: true,
    }
}

fn build_summary_output(
    tool_name: &str,
    args: &serde_json::Value,
    raw_text: &str,
    is_error: bool,
    raw_chars: usize,
    storage_ref: Option<ToolStorageRef>,
    config: &NormalizationConfig,
) -> NormalizedToolOutput {
    let summary = generate_summary(tool_name, args, raw_text, is_error);
    let display_text = truncate_text(&summary, config.display_limit);

    NormalizedToolOutput {
        raw_content: None,
        observation_text: summary.clone(),
        display_text,
        storage_ref,
        strategy: ToolOutputStrategy::SummaryOnly,
        raw_chars,
        observation_chars: summary.chars().count(),
        truncated: true,
    }
}

fn build_file_ref_output(
    _tool_name: &str,
    raw_text: &str,
    raw_chars: usize,
    strategy: ToolOutputStrategy,
    storage_ref: Option<ToolStorageRef>,
    remaining_budget: usize,
    config: &NormalizationConfig,
) -> NormalizedToolOutput {
    let excerpt_limit = remaining_budget.min(config.excerpt_limit);
    let excerpt = if strategy == ToolOutputStrategy::FileRefWithExcerpt && excerpt_limit > 0 {
        Some(truncate_text_exact(raw_text, excerpt_limit))
    } else {
        None
    };

    let storage_hint = storage_ref
        .as_ref()
        .map(|storage| format!("Full output saved to: {}", storage.path.display()))
        .unwrap_or_else(|| "Full output saved to file storage.".to_string());

    let mut observation_text = format!("Output too large ({} chars). {}", raw_chars, storage_hint);
    if let Some(ref excerpt_text) = excerpt {
        observation_text.push_str("\nExcerpt:\n");
        observation_text.push_str(excerpt_text);
    } else if storage_ref.is_none() {
        observation_text.push_str("\nUse read tool to view full output.");
    }

    let display_text = if let Some(ref excerpt_text) = excerpt {
        truncate_text(
            &format!("Saved large output. Excerpt: {}", excerpt_text),
            config.display_limit,
        )
    } else if let Some(ref storage) = storage_ref {
        truncate_text(
            &format!("Saved large output to {}", storage.path.display()),
            config.display_limit,
        )
    } else {
        format!("Large output ({} chars)", raw_chars)
    };

    NormalizedToolOutput {
        raw_content: None,
        observation_text: observation_text.clone(),
        display_text,
        storage_ref,
        strategy,
        raw_chars,
        observation_chars: observation_text.chars().count(),
        truncated: true,
    }
}

/// Determine the appropriate normalization strategy.
fn determine_strategy(
    tool_name: &str,
    raw_chars: usize,
    is_error: bool,
    output_hint: Option<&ToolOutputHint>,
    remaining_budget: usize,
    config: &NormalizationConfig,
) -> ToolOutputStrategy {
    if let Some(hint) = output_hint {
        let safe_inline_limit = hint.safe_inline_chars.unwrap_or(config.inline_limit);
        if raw_chars <= safe_inline_limit && remaining_budget >= raw_chars {
            return ToolOutputStrategy::Inline;
        }

        if let Some(preferred_strategy) = hint.preferred_strategy {
            if remaining_budget == 0 {
                return if matches!(
                    preferred_strategy,
                    ToolOutputStrategy::FileRef | ToolOutputStrategy::FileRefWithExcerpt
                ) {
                    ToolOutputStrategy::FileRef
                } else {
                    ToolOutputStrategy::SummaryOnly
                };
            }
            return match preferred_strategy {
                ToolOutputStrategy::Inline if raw_chars > safe_inline_limit => {
                    if hint.prefer_head_tail {
                        ToolOutputStrategy::HeadTail
                    } else if raw_chars > config.file_ref_threshold {
                        ToolOutputStrategy::FileRefWithExcerpt
                    } else {
                        ToolOutputStrategy::SummaryOnly
                    }
                }
                ToolOutputStrategy::HeadTail if remaining_budget < config.head_tail_limit / 2 => {
                    if raw_chars > config.file_ref_threshold / 2 {
                        ToolOutputStrategy::FileRefWithExcerpt
                    } else {
                        ToolOutputStrategy::SummaryOnly
                    }
                }
                ToolOutputStrategy::FileRefWithExcerpt if remaining_budget == 0 => {
                    ToolOutputStrategy::FileRef
                }
                other => other,
            };
        }

        if hint.prefer_head_tail && raw_chars > safe_inline_limit && remaining_budget > 0 {
            return if raw_chars > config.file_ref_threshold {
                ToolOutputStrategy::FileRefWithExcerpt
            } else {
                ToolOutputStrategy::HeadTail
            };
        }
    }

    let base_strategy = match tool_name {
        "bash" | "powershell" => {
            if raw_chars <= config.inline_limit {
                ToolOutputStrategy::Inline
            } else if raw_chars <= config.file_ref_threshold {
                ToolOutputStrategy::HeadTail
            } else {
                ToolOutputStrategy::FileRefWithExcerpt
            }
        }
        "web_fetcher" | "mcp_call_tool" => {
            if raw_chars <= config.inline_limit {
                ToolOutputStrategy::Inline
            } else {
                ToolOutputStrategy::FileRefWithExcerpt
            }
        }
        "get_recent_messages" => {
            if raw_chars <= config.inline_limit / 2 {
                ToolOutputStrategy::Inline
            } else {
                ToolOutputStrategy::SummaryOnly
            }
        }
        "invoke_agent" => {
            if raw_chars <= config.inline_limit {
                ToolOutputStrategy::Inline
            } else if raw_chars <= config.file_ref_threshold {
                ToolOutputStrategy::HeadTail
            } else {
                ToolOutputStrategy::FileRefWithExcerpt
            }
        }
        _ => {
            if raw_chars <= config.inline_limit {
                ToolOutputStrategy::Inline
            } else if raw_chars <= config.file_ref_threshold {
                if is_error {
                    ToolOutputStrategy::HeadTail
                } else {
                    ToolOutputStrategy::SummaryOnly
                }
            } else {
                ToolOutputStrategy::FileRefWithExcerpt
            }
        }
    };

    if remaining_budget == 0 {
        return if matches!(
            base_strategy,
            ToolOutputStrategy::FileRef | ToolOutputStrategy::FileRefWithExcerpt
        ) {
            ToolOutputStrategy::FileRef
        } else {
            ToolOutputStrategy::SummaryOnly
        };
    }

    if raw_chars > remaining_budget {
        return match tool_name {
            "bash" | "powershell" if remaining_budget >= config.head_tail_limit / 2 => ToolOutputStrategy::HeadTail,
            "web_fetcher" | "invoke_agent" | "mcp_call_tool" => ToolOutputStrategy::FileRefWithExcerpt,
            "get_recent_messages" => ToolOutputStrategy::SummaryOnly,
            _ if raw_chars > config.file_ref_threshold / 2 => ToolOutputStrategy::FileRefWithExcerpt,
            _ if is_error && remaining_budget >= config.head_tail_limit / 2 => ToolOutputStrategy::HeadTail,
            _ => ToolOutputStrategy::SummaryOnly,
        };
    }

    base_strategy
}

/// Truncate text to a maximum character count, adding ellipsis if needed.
fn truncate_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}

/// Truncate text while guaranteeing the output never exceeds `max_chars`.
fn truncate_text_exact(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }

    if max_chars <= 3 {
        return text.chars().take(max_chars).collect();
    }

    let keep = max_chars - 3;
    let truncated: String = text.chars().take(keep).collect();
    format!("{}...", truncated)
}

/// Split text into head and tail portions.
fn split_head_tail(text: &str, portion_limit: usize) -> (String, String) {
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();

    if total <= portion_limit * 2 {
        return (text.to_string(), String::new());
    }

    let head: String = chars.iter().take(portion_limit).collect();
    let tail: String = chars.iter().skip(total - portion_limit).collect();

    (head, tail)
}

/// Generate a summary for large outputs.
fn generate_summary(
    tool_name: &str,
    _args: &serde_json::Value,
    raw_text: &str,
    is_error: bool,
) -> String {
    let lines: Vec<&str> = raw_text.lines().collect();
    let line_count = lines.len();
    let char_count = raw_text.chars().count();
    let status = if is_error { "error" } else { "success" };

    if line_count <= 5 {
        format!(
            "Tool {} ({}, {} lines, {} chars):\n{}",
            tool_name, status, line_count, char_count, raw_text
        )
    } else {
        let preview: Vec<&str> = lines.iter().take(3).copied().collect();
        format!(
            "Tool {} ({}, {} lines, {} chars).\nFirst lines:\n{}",
            tool_name,
            status,
            line_count,
            char_count,
            preview.join("\n")
        )
    }
}

fn apply_observation_budget(output: &mut NormalizedToolOutput, remaining_budget: usize) {
    if output.observation_chars <= remaining_budget {
        return;
    }

    output.observation_text = truncate_text_exact(&output.observation_text, remaining_budget);
    output.observation_chars = output.observation_text.chars().count();
    output.truncated = true;
    output.strategy = if output.storage_ref.is_some() {
        ToolOutputStrategy::FileRefWithExcerpt
    } else {
        ToolOutputStrategy::SummaryOnly
    };
}

fn persist_output(
    tool_name: &str,
    args: &serde_json::Value,
    raw_text: &str,
    config: &NormalizationConfig,
) -> Option<ToolStorageRef> {
    if !config.enable_persistence {
        return None;
    }

    let storage_dir = config
        .storage_dir
        .clone()
        .unwrap_or_else(default_tool_output_dir);
    std::fs::create_dir_all(&storage_dir).ok()?;

    let (extension, content_type) = infer_storage_format(tool_name, args, raw_text);
    let file_name = build_storage_file_name(tool_name, extension);
    let path = storage_dir.join(file_name);
    std::fs::write(&path, raw_text).ok()?;

    Some(ToolStorageRef {
        path,
        size: raw_text.len(),
        content_type: content_type.to_string(),
        encoding: "utf-8".to_string(),
        tool_name: tool_name.to_string(),
    })
}

fn default_tool_output_dir() -> PathBuf {
    env_config::home::thread_session_dir("default").join("tool-output")
}

fn build_storage_file_name(tool_name: &str, extension: &str) -> String {
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let unique = TOOL_OUTPUT_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{}-{}-{:08x}.{}",
        sanitize_file_component(tool_name),
        timestamp,
        unique,
        extension
    )
}

fn sanitize_file_component(value: &str) -> String {
    let mut sanitized = value
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    while sanitized.contains("--") {
        sanitized = sanitized.replace("--", "-");
    }
    let trimmed = sanitized.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "tool".to_string()
    } else {
        trimmed
    }
}

fn infer_storage_format(
    tool_name: &str,
    args: &serde_json::Value,
    raw_text: &str,
) -> (&'static str, &'static str) {
    if tool_name == "bash" || tool_name == "powershell" {
        return ("log", "text/plain");
    }

    if looks_like_json(raw_text) {
        return ("json", "application/json");
    }

    if looks_like_html(raw_text) {
        return ("html", "text/html");
    }

    if tool_name == "web_fetcher" {
        if let Some(url) = args.get("url").and_then(|v| v.as_str()) {
            if let Some(ext) = Path::new(url)
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_lowercase())
            {
                match ext.as_str() {
                    "json" => return ("json", "application/json"),
                    "html" | "htm" => return ("html", "text/html"),
                    "xml" => return ("xml", "application/xml"),
                    "md" => return ("md", "text/markdown"),
                    _ => {}
                }
            }
        }
    }

    ("txt", "text/plain")
}

fn looks_like_json(raw_text: &str) -> bool {
    let trimmed = raw_text.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

fn looks_like_html(raw_text: &str) -> bool {
    let trimmed = raw_text.trim_start().to_ascii_lowercase();
    trimmed.starts_with("<!doctype html")
        || trimmed.starts_with("<html")
        || trimmed.contains("<body")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_truncate_text_short() {
        let text = "hello";
        let result = truncate_text(text, 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_text_long() {
        let text = "a".repeat(50);
        let result = truncate_text(&text, 10);
        assert!(result.ends_with("..."));
        assert_eq!(result.chars().count(), 13);
    }

    #[test]
    fn test_truncate_text_exact_respects_limit() {
        let text = "abcdef";
        assert_eq!(truncate_text_exact(text, 0), "");
        assert_eq!(truncate_text_exact(text, 2).chars().count(), 2);
        assert_eq!(truncate_text_exact(text, 4).chars().count(), 4);
    }

    #[test]
    fn test_split_head_tail_small() {
        let text = "hello world";
        let (head, tail) = split_head_tail(text, 100);
        assert_eq!(head, "hello world");
        assert!(tail.is_empty());
    }

    #[test]
    fn test_split_head_tail_large() {
        let text = "a".repeat(1000);
        let (head, tail) = split_head_tail(&text, 100);
        assert_eq!(head.chars().count(), 100);
        assert_eq!(tail.chars().count(), 100);
    }

    #[test]
    fn test_normalize_inline() {
        let text = "small output";
        let result = normalize_tool_output(
            "test_tool",
            &json!({}),
            text,
            false,
            None,
            NormalizationConfig::default(),
        );

        assert_eq!(result.strategy, ToolOutputStrategy::Inline);
        assert_eq!(result.raw_content.as_deref(), Some(text));
        assert_eq!(result.observation_text, text);
        assert!(!result.truncated);
    }

    #[test]
    fn test_normalize_bash_head_tail() {
        // Need > inline_limit (4000) to get HeadTail; 700 * 6 = 4200
        let text = "line1\n".repeat(700);
        let result = normalize_tool_output(
            "bash",
            &json!({"command": "test"}),
            &text,
            false,
            None,
            NormalizationConfig::default(),
        );

        assert_eq!(result.strategy, ToolOutputStrategy::HeadTail);
        assert!(result.truncated);
        assert!(result.observation_text.contains("Head:"));
    }

    #[test]
    fn test_normalize_get_recent_messages_summary() {
        // get_recent_messages uses inline_limit/2 (2000); need > 2000. 300 * 9 = 2700
        let text = "message\n".repeat(300);
        let result = normalize_tool_output(
            "get_recent_messages",
            &json!({"limit": 100}),
            &text,
            false,
            None,
            NormalizationConfig::default(),
        );

        assert_eq!(result.strategy, ToolOutputStrategy::SummaryOnly);
        assert!(result.truncated);
        assert!(result.observation_text.contains("get_recent_messages"));
    }

    #[test]
    fn test_normalize_very_large_file_ref() {
        let text = "x".repeat(20_000);
        let result = normalize_tool_output(
            "web_fetcher",
            &json!({"url": "http://example.com"}),
            &text,
            false,
            None,
            NormalizationConfig::default(),
        );

        assert_eq!(result.strategy, ToolOutputStrategy::FileRefWithExcerpt);
        assert!(result.truncated);
        // Format uses raw number (e.g. "20000 chars"), not comma-separated
        assert!(result.observation_text.contains("20000 chars"));
    }

    #[test]
    fn test_normalize_error_gets_head_tail() {
        // Unknown tool + error: need > inline_limit (4000) for HeadTail. 400 * 11 = 4400
        let text = "error line\n".repeat(400);
        let result = normalize_tool_output(
            "unknown_tool",
            &json!({}),
            &text,
            true,
            None,
            NormalizationConfig::default(),
        );

        assert_eq!(result.strategy, ToolOutputStrategy::HeadTail);
        assert!(result.truncated);
    }

    #[test]
    fn test_budget_degrades_large_output() {
        let text = "x".repeat(4_500);
        let config = NormalizationConfig::default().with_used_observation_chars(7_900);
        let result = normalize_tool_output("bash", &json!({}), &text, false, None, config);
        assert!(result.observation_chars <= 100);
        assert!(matches!(
            result.strategy,
            ToolOutputStrategy::SummaryOnly | ToolOutputStrategy::FileRefWithExcerpt
        ));
    }

    #[test]
    fn test_generate_summary_small() {
        let text = "line1\nline2";
        let summary = generate_summary("test", &json!({}), text, false);
        assert!(summary.contains("test"));
        assert!(summary.contains("success"));
        assert!(summary.contains("line1"));
    }

    #[test]
    fn test_generate_summary_large() {
        let text = (1..=20)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let summary = generate_summary("test", &json!({}), &text, true);
        assert!(summary.contains("test"));
        assert!(summary.contains("error"));
        assert!(summary.contains("20 lines"));
        assert!(summary.contains("line1"));
        assert!(!summary.contains("line20"));
    }
}
