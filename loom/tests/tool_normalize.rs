//! Integration tests for the public tool output normalization API.

use loom::{normalize_tool_output, NormalizationConfig, ToolOutputHint, ToolOutputStrategy};
use serde_json::json;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn make_long_string(n_chars: usize) -> String {
    "x".repeat(n_chars)
}

fn with_temp_loom_home<F: FnOnce()>(f: F) {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let prev = std::env::var("LOOM_HOME").ok();
    std::env::set_var("LOOM_HOME", dir.path());
    f();
    match prev {
        Some(v) => std::env::set_var("LOOM_HOME", v),
        None => std::env::remove_var("LOOM_HOME"),
    }
}

#[test]
fn small_output_uses_inline_strategy() {
    let text = "Hello, world!";
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
    assert_eq!(result.display_text, text);
    assert!(!result.truncated);
    assert!(result.storage_ref.is_none());
}

#[test]
fn medium_bash_output_uses_head_tail_strategy() {
    let config = NormalizationConfig::default();
    let text = "line1\n".repeat(900);
    let result = normalize_tool_output(
        "bash",
        &json!({"command": "test"}),
        &text,
        false,
        None,
        config,
    );

    assert_eq!(result.strategy, ToolOutputStrategy::HeadTail);
    assert!(result.truncated);
    assert!(result.raw_content.is_some());
    assert!(result.observation_text.contains("..."));
}

#[test]
fn get_recent_messages_large_output_uses_inline() {
    let text = (0..200)
        .map(|i| format!("Message {}: {}", i, "x".repeat(40)))
        .collect::<Vec<_>>()
        .join("\n");
    let result = normalize_tool_output(
        "get_recent_messages",
        &json!({"limit": 100}),
        &text,
        false,
        None,
        NormalizationConfig::default(),
    );

    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
    assert!(result.raw_content.is_some());
    assert_eq!(result.observation_text, text);
}

#[test]
fn very_large_web_fetcher_output_uses_file_ref_with_excerpt() {
    with_temp_loom_home(|| {
        let text = make_long_string(20_000);
        let result = normalize_tool_output(
            "web_fetcher",
            &json!({"url": "https://example.com"}),
            &text,
            false,
            None,
            NormalizationConfig::runtime_default(),
        );

        assert_eq!(result.strategy, ToolOutputStrategy::FileRefWithExcerpt);
        assert!(result.truncated);
        assert!(result.raw_content.is_none());
        assert!(
            result.storage_ref.is_some(),
            "large output should be persisted"
        );
        assert!(result.observation_text.contains("Output too large"));
        assert!(result.observation_text.contains("Excerpt:"));

        let storage_ref = result.storage_ref.unwrap();
        assert!(storage_ref.path.exists());
        assert_eq!(std::fs::read_to_string(&storage_ref.path).unwrap(), text);
    });
}

#[test]
fn chars_tracking_uses_character_counts() {
    let text = "Hello 世界";
    let result = normalize_tool_output(
        "test_tool",
        &json!({}),
        text,
        false,
        None,
        NormalizationConfig::default(),
    );

    assert_eq!(result.raw_chars, text.chars().count());
    assert_eq!(
        result.observation_chars,
        result.observation_text.chars().count()
    );
}

#[test]
fn large_output_is_downgraded_when_turn_budget_is_nearly_spent() {
    let text = make_long_string(4_500);
    let result = normalize_tool_output(
        "bash",
        &json!({"command": "cargo test"}),
        &text,
        false,
        None,
        NormalizationConfig::default().with_used_observation_chars(7_950),
    );

    assert!(result.observation_chars <= 50);
    assert!(matches!(
        result.strategy,
        ToolOutputStrategy::SummaryOnly | ToolOutputStrategy::FileRefWithExcerpt
    ));
}

#[test]
fn tool_output_hint_is_ignored_for_non_whitelisted_tool() {
    let text = make_long_string(6_000);
    let hint = ToolOutputHint::preferred(ToolOutputStrategy::HeadTail).prefer_head_tail();
    let result = normalize_tool_output(
        "custom_tool",
        &json!({}),
        &text,
        false,
        Some(&hint),
        NormalizationConfig::default(),
    );

    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
    assert_eq!(result.observation_text, text);
}

#[test]
fn web_search_large_output_uses_file_ref_with_excerpt() {
    with_temp_loom_home(|| {
        let text = make_long_string(20_000);
        let result = normalize_tool_output(
            "web_search",
            &json!({"query": "test"}),
            &text,
            false,
            None,
            NormalizationConfig::runtime_default(),
        );

        assert_eq!(result.strategy, ToolOutputStrategy::FileRefWithExcerpt);
        assert!(result.truncated);
        assert!(result.storage_ref.is_some());
    });
}

#[test]
fn web_search_small_output_uses_inline() {
    let text = "3 results found";
    let result = normalize_tool_output(
        "web_search",
        &json!({"query": "test"}),
        text,
        false,
        None,
        NormalizationConfig::default(),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
}

#[test]
fn read_large_output_uses_inline() {
    let text = make_long_string(20_000);
    let result = normalize_tool_output(
        "read",
        &json!({"path": "/tmp/big.txt"}),
        &text,
        false,
        None,
        NormalizationConfig::default(),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
    assert_eq!(result.observation_text, text);
}

#[test]
fn grep_large_output_uses_inline() {
    let text = make_long_string(20_000);
    let result = normalize_tool_output(
        "grep",
        &json!({"pattern": "test"}),
        &text,
        false,
        None,
        NormalizationConfig::default(),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
}

#[test]
fn invoke_agent_large_output_uses_inline() {
    let text = make_long_string(20_000);
    let result = normalize_tool_output(
        "invoke_agent",
        &json!({"agent": "explore", "task": "..."}),
        &text,
        false,
        None,
        NormalizationConfig::default(),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
}

#[test]
fn unknown_tool_error_output_uses_inline() {
    let text = make_long_string(5_000);
    let result = normalize_tool_output(
        "unknown_tool",
        &json!({}),
        &text,
        true,
        None,
        NormalizationConfig::default(),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
    assert_eq!(result.observation_text, text);
}

#[test]
fn non_whitelisted_tool_ignores_safe_inline_chars_hint() {
    let text = make_long_string(500);
    let hint =
        ToolOutputHint::preferred(ToolOutputStrategy::HeadTail).safe_inline_chars(100);
    let result = normalize_tool_output(
        "read",
        &json!({}),
        &text,
        false,
        Some(&hint),
        NormalizationConfig::default(),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
}

#[test]
fn whitelisted_tool_respects_safe_inline_chars_hint() {
    let text = make_long_string(500);
    let hint =
        ToolOutputHint::preferred(ToolOutputStrategy::HeadTail).safe_inline_chars(100);
    let result = normalize_tool_output(
        "bash",
        &json!({"command": "test"}),
        &text,
        false,
        Some(&hint),
        NormalizationConfig::default(),
    );
    assert_ne!(result.strategy, ToolOutputStrategy::Inline);
    assert!(result.truncated);
}

#[test]
fn non_whitelisted_tool_ignores_observation_budget() {
    let text = make_long_string(20_000);
    let result = normalize_tool_output(
        "read",
        &json!({}),
        &text,
        false,
        None,
        NormalizationConfig::default().with_used_observation_chars(7_950),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
    assert_eq!(result.observation_text, text);
}

#[test]
fn whitelisted_tool_degrades_when_budget_spent() {
    let text = make_long_string(4_500);
    let result = normalize_tool_output(
        "bash",
        &json!({"command": "test"}),
        &text,
        false,
        None,
        NormalizationConfig::default().with_used_observation_chars(7_950),
    );
    assert!(matches!(
        result.strategy,
        ToolOutputStrategy::SummaryOnly | ToolOutputStrategy::FileRefWithExcerpt
    ));
}
