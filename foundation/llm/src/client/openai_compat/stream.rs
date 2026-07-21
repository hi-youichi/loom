//! Stream chunk DTOs for SSE parsing.
//!
//! These types model the JSON shape of individual `data:` lines in the
//! Server-Sent Events stream returned by OpenAI-compatible providers.

/// Function fragment inside a streamed tool-call delta.
#[derive(serde::Deserialize, Default)]
pub(super) struct StreamDeltaFunction {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

/// A single tool-call delta in a stream chunk.
#[derive(serde::Deserialize, Default)]
pub(super) struct StreamToolCallDelta {
    pub index: u32,
    pub id: Option<String>,
    pub function: Option<StreamDeltaFunction>,
}

/// Content delta for one choice in a stream chunk.
#[derive(serde::Deserialize, Default)]
pub(super) struct StreamDelta {
    pub content: Option<String>,
    #[serde(default,
        alias = "reasoning",
        alias = "reason_content",
        alias = "thinking",                // Anthropic extended thinking / Stepfun / Qwen QwQ
        alias = "reasoning_text",          // DeepSeek R1 / GLM-4.5 z.ai / OpenRouter
        alias = "reasoning_details",       // 一些反代 / provider-options 层
    )]
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<StreamToolCallDelta>>,
}

/// One choice inside a stream chunk.
#[derive(serde::Deserialize)]
pub(super) struct StreamChoice {
    pub delta: StreamDelta,
    /// OpenAI-compatible; optional so we don't fail if the API omits it.
    #[serde(default)]
    #[allow(dead_code)]
    pub finish_reason: Option<String>,
}

/// Top-level SSE chunk from `/chat/completions` with `stream: true`.
#[derive(serde::Deserialize)]
pub(super) struct StreamChunk {
    pub choices: Option<Vec<StreamChoice>>,
    pub usage: Option<super::request::ResponseUsage>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P0 #1: providers express reasoning under many field names. Verify
    /// every supported alias lands on `reasoning_content` so the
    /// `MessageChunk::thinking` emit (see `llm_client.rs`) is actually
    /// triggered.
    #[test]
    fn reasoning_aliases_all_land_on_reasoning_content() {
        let cases: Vec<(&str, &str, &str)> = vec![
            ("reasoning_content", r#"{"content":null,"reasoning_content":"rc"}"#, "rc"),
            ("reasoning", r#"{"content":null,"reasoning":"r"}"#, "r"),
            ("reason_content", r#"{"content":null,"reason_content":"rcnt"}"#, "rcnt"),
            ("thinking", r#"{"content":null,"thinking":"th"}"#, "th"),
            ("reasoning_text", r#"{"content":null,"reasoning_text":"rt"}"#, "rt"),
            ("reasoning_details", r#"{"content":null,"reasoning_details":"rd"}"#, "rd"),
        ];
        for (alias, raw, expected) in cases {
            let delta: StreamDelta = serde_json::from_str(raw)
                .unwrap_or_else(|e| panic!("field `{alias}` must deserialize: {e}"));
            assert_eq!(
                delta.reasoning_content.as_deref(),
                Some(expected),
                "field `{alias}` must populate `reasoning_content` with `{expected}`"
            );
        }
    }

    /// When reasoning is absent, `reasoning_content` must be `None` (not an
    /// error, not a default empty string) so the `if !is_empty()` gate
    /// in `llm_client.rs:387-398` correctly suppresses spurious chunks.
    #[test]
    fn no_reasoning_field_leaves_reasoning_content_none() {
        let delta: StreamDelta = serde_json::from_str(r#"{"content":"hello"}"#).unwrap();
        assert_eq!(delta.reasoning_content, None);
        assert_eq!(delta.content.as_deref(), Some("hello"));
    }

    /// Regression: a non-reasoning alias we did NOT whitelist must NOT
    /// silently populate `reasoning_content` — that would mask upstream
    /// protocol drift. If this test starts failing, an unknown field has
    /// leaked through; check that the alias above is the legitimate
    /// addition (not a typo).
    #[test]
    fn unknown_reasoning_alias_is_ignored() {
        let delta: StreamDelta =
            serde_json::from_str(r#"{"content":null,"not_a_real_field":"x"}"#).unwrap();
        assert_eq!(
            delta.reasoning_content, None,
            "unknown alias must not silently populate reasoning_content"
        );
    }
}

