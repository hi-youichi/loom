//! Unit tests for run config summary types (ConfigSection, RunConfigSummary, *ConfigSummary).
//!
//! Verifies section_name() and entries() for LlmConfigSummary, MemoryConfigSummary,
//! ToolConfigSummary, EmbeddingConfigSummary; and RunConfigSummary builder order.

mod init_logging;

use graphweave::{
    ConfigSection, EmbeddingConfigSummary, LlmConfigSummary, MemoryConfigSummary, RunConfigSummary,
    ToolConfigSummary,
};

// --- LlmConfigSummary ---

/// Given an LlmConfigSummary with model and api_base set, section_name returns "LLM config".
#[test]
fn llm_config_summary_section_name_is_llm_config() {
    let s = LlmConfigSummary {
        model: "gpt-4o-mini".into(),
        api_base: "https://api.openai.com/v1".into(),
        temperature: None,
        tool_choice: "auto".into(),
    };
    assert_eq!(s.section_name(), "LLM config");
}

/// Given temperature None, entries show temperature=(default).
#[test]
fn llm_config_summary_entries_temperature_default_when_none() {
    let s = LlmConfigSummary {
        model: "gpt-4o-mini".into(),
        api_base: "https://api.openai.com/v1".into(),
        temperature: None,
        tool_choice: "auto".into(),
    };
    let entries = s.entries();
    let map: std::collections::HashMap<_, _> = entries.into_iter().collect();
    assert_eq!(map.get("model").map(|v| v.as_str()), Some("gpt-4o-mini"));
    assert_eq!(
        map.get("temperature").map(|v| v.as_str()),
        Some("(default)")
    );
    assert_eq!(map.get("tool_choice").map(|v| v.as_str()), Some("auto"));
}

/// Given temperature Some(0.2), entries show temperature=0.2.
#[test]
fn llm_config_summary_entries_temperature_value_when_some() {
    let s = LlmConfigSummary {
        model: "gpt-4o".into(),
        api_base: "https://api.openai.com/v1".into(),
        temperature: Some(0.2),
        tool_choice: "none".into(),
    };
    let entries = s.entries();
    let map: std::collections::HashMap<_, _> = entries.into_iter().collect();
    assert_eq!(map.get("temperature").map(|v| v.as_str()), Some("0.2"));
}

// --- MemoryConfigSummary ---

/// Given mode=both with short_term and long_term, section_name is "Memory config" and entries include mode, short_term, store.
#[test]
fn memory_config_summary_section_name_and_entries_both() {
    let s = MemoryConfigSummary {
        mode: "both".into(),
        short_term: Some("sqlite".into()),
        thread_id: Some("t1".into()),
        db_path: Some("memory.db".into()),
        long_term: Some("vector".into()),
        long_term_store: Some("in_memory_vector".into()),
    };
    assert_eq!(s.section_name(), "Memory config");
    let entries = s.entries();
    let map: std::collections::HashMap<_, _> = entries.into_iter().collect();
    assert_eq!(map.get("mode").map(|v| v.as_str()), Some("both"));
    assert_eq!(map.get("short_term").map(|v| v.as_str()), Some("sqlite"));
    assert_eq!(
        map.get("store").map(|v| v.as_str()),
        Some("in_memory_vector")
    );
}

/// Given mode=none, entries contain only mode.
#[test]
fn memory_config_summary_entries_mode_none() {
    let s = MemoryConfigSummary {
        mode: "none".into(),
        short_term: None,
        thread_id: None,
        db_path: None,
        long_term: None,
        long_term_store: None,
    };
    let entries = s.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, "mode");
    assert_eq!(entries[0].1, "none");
}

// --- ToolConfigSummary ---

/// Given sources and exa_url, section_name is "Tools" and entries have tools and exa_url.
#[test]
fn tool_config_summary_section_name_and_entries_with_exa() {
    let s = ToolConfigSummary {
        sources: vec!["memory".into(), "exa".into()],
        exa_url: Some("https://mcp.exa.ai/mcp".into()),
    };
    assert_eq!(s.section_name(), "Tools");
    let entries = s.entries();
    let map: std::collections::HashMap<_, _> = entries.into_iter().collect();
    assert_eq!(map.get("tools").map(|v| v.as_str()), Some("memory,exa"));
    assert_eq!(
        map.get("exa_url").map(|v| v.as_str()),
        Some("https://mcp.exa.ai/mcp")
    );
}

// --- EmbeddingConfigSummary ---

/// Given model and api_base, section_name is "Embedding" and entries contain model and api_base.
#[test]
fn embedding_config_summary_section_name_and_entries() {
    let s = EmbeddingConfigSummary {
        model: "text-embedding-3-small".into(),
        api_base: "https://api.openai.com/v1".into(),
    };
    assert_eq!(s.section_name(), "Embedding");
    let entries = s.entries();
    let map: std::collections::HashMap<_, _> = entries.into_iter().collect();
    assert_eq!(
        map.get("model").map(|v| v.as_str()),
        Some("text-embedding-3-small")
    );
    assert_eq!(
        map.get("api_base").map(|v| v.as_str()),
        Some("https://api.openai.com/v1")
    );
}

// --- RunConfigSummary ---

/// Given with_section called in order, sections().len() matches and order is preserved.
#[test]
fn run_config_summary_with_section_preserves_order_and_count() {
    let llm = LlmConfigSummary {
        model: "m".into(),
        api_base: "b".into(),
        temperature: None,
        tool_choice: "auto".into(),
    };
    let summary = RunConfigSummary::new()
        .with_section(Box::new(llm))
        .with_section(Box::new(EmbeddingConfigSummary {
            model: "e".into(),
            api_base: "e-b".into(),
        }));
    assert_eq!(summary.sections().len(), 2);
    assert_eq!(summary.sections()[0].section_name(), "LLM config");
    assert_eq!(summary.sections()[1].section_name(), "Embedding");
}
