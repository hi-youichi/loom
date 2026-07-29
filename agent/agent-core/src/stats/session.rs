//! Public session stats types for the `stats` module.
//!
//! Phase 1 surface only — see PLAN §3. SQLite persistence, query API, and
//! wire-in to `AgentRunner` are Phase 2 concerns.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SessionStatus {
    #[default]
    Active,
    Completed,
    Errored,
    Cancelled,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Errored => "errored",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "completed" => Some(Self::Completed),
            "errored" => Some(Self::Errored),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ErrorKind {
    #[default]
    Llm,
    Tool,
    Agent,
    Cancel,
}

impl ErrorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Llm => "llm",
            Self::Tool => "tool",
            Self::Agent => "agent",
            Self::Cancel => "cancel",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        match s {
            "llm" => Some(Self::Llm),
            "tool" => Some(Self::Tool),
            "agent" => Some(Self::Agent),
            "cancel" => Some(Self::Cancel),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub status: SessionStatus,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_cached_tokens: u64,
    pub total_tokens: u64,
    pub llm_call_count: u64,
    pub tool_call_count: u64,
    pub error_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionStats {
    pub summary: SessionSummary,
    pub llm_calls: Vec<LlmCallRecord>,
    pub tool_calls: Vec<ToolCallRecord>,
    pub errors: Vec<ErrorRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmCallRecord {
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub latency_ms: i64,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cached_tokens: Option<u32>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub latency_ms: i64,
    pub name: String,
    pub arguments: String,
    pub result_size: usize,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorRecord {
    pub at_ms: i64,
    pub kind: ErrorKind,
    pub message: String,
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    pub total_tokens: u64,
    pub call_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolUsage {
    pub name: String,
    pub call_count: u64,
    pub error_count: u64,
    pub total_latency_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_status_roundtrip() {
        for variant in [
            SessionStatus::Active,
            SessionStatus::Completed,
            SessionStatus::Errored,
            SessionStatus::Cancelled,
        ] {
            let s = variant.as_str();
            assert_eq!(SessionStatus::from_label(s), Some(variant));
        }
    }

    #[test]
    fn session_status_unknown_returns_none() {
        assert_eq!(SessionStatus::from_label("unknown"), None);
        assert_eq!(SessionStatus::from_label(""), None);
        assert_eq!(SessionStatus::from_label("ACTIVE"), None);
    }

    #[test]
    fn error_kind_roundtrip() {
        for variant in [
            ErrorKind::Llm,
            ErrorKind::Tool,
            ErrorKind::Agent,
            ErrorKind::Cancel,
        ] {
            let s = variant.as_str();
            assert_eq!(ErrorKind::from_label(s), Some(variant));
        }
    }

    #[test]
    fn error_kind_unknown_returns_none() {
        assert_eq!(ErrorKind::from_label("unknown"), None);
        assert_eq!(ErrorKind::from_label(""), None);
        assert_eq!(ErrorKind::from_label("LLM"), None);
    }

    #[test]
    fn summary_default_is_zero() {
        let s = SessionSummary::default();
        assert_eq!(s.session_id, "");
        assert_eq!(s.started_at_ms, 0);
        assert_eq!(s.ended_at_ms, None);
        assert_eq!(s.status, SessionStatus::Active);
        assert_eq!(s.total_prompt_tokens, 0);
        assert_eq!(s.total_completion_tokens, 0);
        assert_eq!(s.total_cached_tokens, 0);
        assert_eq!(s.total_tokens, 0);
        assert_eq!(s.llm_call_count, 0);
        assert_eq!(s.tool_call_count, 0);
        assert_eq!(s.error_count, 0);
    }

    #[test]
    #[allow(dead_code)]
    fn summary_manual_construction_matches_default() {
        let s = SessionSummary {
            session_id: String::new(),
            started_at_ms: 0,
            ended_at_ms: None,
            status: SessionStatus::Active,
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
            total_cached_tokens: 0,
            total_tokens: 0,
            llm_call_count: 0,
            tool_call_count: 0,
            error_count: 0,
        };
        assert_eq!(s, SessionSummary::default());
    }

    #[test]
    fn serde_roundtrip_session_stats() {
        let stats = SessionStats {
            summary: SessionSummary {
                session_id: "sess_42".into(),
                started_at_ms: 1_700_000_000_000,
                ended_at_ms: Some(1_700_000_500_000),
                status: SessionStatus::Completed,
                total_prompt_tokens: 1234,
                total_completion_tokens: 567,
                total_cached_tokens: 89,
                total_tokens: 1801,
                llm_call_count: 4,
                tool_call_count: 2,
                error_count: 1,
            },
            llm_calls: vec![LlmCallRecord {
                started_at_ms: 1_700_000_001_000,
                ended_at_ms: 1_700_000_002_500,
                latency_ms: 1500,
                prompt_tokens: 500,
                completion_tokens: 200,
                total_tokens: 700,
                cached_tokens: Some(120),
                model: None,
            }],
            tool_calls: vec![ToolCallRecord {
                started_at_ms: 1_700_000_010_000,
                ended_at_ms: 1_700_000_010_300,
                latency_ms: 300,
                name: "bash".into(),
                arguments: "{}".into(),
                result_size: 12,
                is_error: false,
            }],
            errors: vec![ErrorRecord {
                at_ms: 1_700_000_010_300,
                kind: ErrorKind::Tool,
                message: "tool bash failed".into(),
                tool_name: Some("bash".into()),
            }],
        };

        let json = serde_json::to_string(&stats).expect("serialize");
        let back: SessionStats = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, stats);
    }

    #[test]
    fn serde_roundtrip_llm_record_with_optional_model() {
        let none_record = LlmCallRecord {
            started_at_ms: 1,
            ended_at_ms: 2,
            latency_ms: 1,
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
            cached_tokens: None,
            model: None,
        };
        let some_record = LlmCallRecord {
            model: Some("claude-sonnet".into()),
            ..none_record.clone()
        };

        let none_json = serde_json::to_string(&none_record).expect("serialize");
        let some_json = serde_json::to_string(&some_record).expect("serialize");

        let none_back: LlmCallRecord = serde_json::from_str(&none_json).expect("deserialize");
        let some_back: LlmCallRecord = serde_json::from_str(&some_json).expect("deserialize");

        assert_eq!(none_back, none_record);
        assert_eq!(some_back, some_record);
        assert_eq!(none_back.model, None);
        assert_eq!(some_back.model.as_deref(), Some("claude-sonnet"));
    }
}