//! OpenAI `/v1/models` listing and permissive deserialization.

use async_openai::config::Config;

use crate::error::AgentError;
use crate::llm::ModelInfo;

/// Fetch models from the provider's `/models` endpoint.
///
/// Uses `reqwest` directly (not `async_openai`) because some gateways
/// omit `created` and `async_openai::Model` fails to deserialize.
pub(super) async fn list_models(config: &impl Config) -> Result<Vec<ModelInfo>, AgentError> {
    let url = config.url("/models");
    let res = reqwest::Client::new()
        .get(&url)
        .headers(config.headers())
        .send()
        .await
        .map_err(|e| AgentError::ExecutionFailed(format!("Failed to list models: {}", e)))?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(AgentError::ExecutionFailed(format!(
            "Failed to list models: {} - {}",
            status, body
        )));
    }

    let body = res
        .text()
        .await
        .map_err(|e| AgentError::ExecutionFailed(format!("Failed to list models: {}", e)))?;

    let parsed: OpenAiListModelsBody = serde_json::from_str(&body).map_err(|e| {
        AgentError::ExecutionFailed(format!(
            "Failed to list models: failed to deserialize api response: {} content:{}",
            e, body
        ))
    })?;

    Ok(parsed
        .data
        .into_iter()
        .map(|m| ModelInfo {
            id: m.id,
            created: m.created,
            owned_by: m.owned_by,
        })
        .collect())
}

/// `/v1/models` list payload: tolerate missing `created` and other gateway quirks.
#[derive(serde::Deserialize)]
struct OpenAiListModelsBody {
    data: Vec<OpenAiModelListRow>,
}

#[derive(serde::Deserialize)]
struct OpenAiModelListRow {
    id: String,
    #[serde(default, deserialize_with = "deserialize_optional_model_created")]
    created: Option<i64>,
    #[serde(default)]
    owned_by: Option<String>,
}

fn deserialize_optional_model_created<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Deserialize;
    let v: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    Ok(v.and_then(|v| match v {
        serde_json::Value::Number(n) => n.as_i64().or_else(|| n.as_u64().map(|u| u as i64)),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_list_models_body_allows_missing_created() {
        let json = r#"{"data":[{"id":"chatgpt-4o-latest","object":"model","owned_by":"openai","permission":[],"root":"chatgpt-4o-latest","parent":null}]}"#;
        let parsed: OpenAiListModelsBody = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].id, "chatgpt-4o-latest");
        assert_eq!(parsed.data[0].created, None);
        assert_eq!(parsed.data[0].owned_by.as_deref(), Some("openai"));
    }

    #[test]
    fn openai_list_models_body_parses_created_number_or_string() {
        let with_num = r#"{"data":[{"id":"a","created":1700000000}]}"#;
        let p: OpenAiListModelsBody = serde_json::from_str(with_num).unwrap();
        assert_eq!(p.data[0].created, Some(1_700_000_000));

        let with_str = r#"{"data":[{"id":"b","created":"1700000001"}]}"#;
        let p2: OpenAiListModelsBody = serde_json::from_str(with_str).unwrap();
        assert_eq!(p2.data[0].created, Some(1_700_000_001));
    }
}
