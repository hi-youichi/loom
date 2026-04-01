use std::collections::HashMap;

use serde_json::Value;

use crate::spec::{Cost, Modalities, ModalityType, Model, ModelLimit, Provider};

/// Parse Provider from JSON
pub fn parse_provider(provider_id: &str, value: &Value) -> Option<Provider> {
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(provider_id)
        .to_string();

    let env = value
        .get("env")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let npm = value
        .get("npm")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let doc = value
        .get("doc")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let api = value
        .get("api")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let models = value
        .get("models")
        .and_then(|v| v.as_object())
        .iter()
        .flat_map(|models| models.iter())
        .filter_map(|(model_id, model_value)| {
            parse_model(model_id, model_value).map(|model| (model_id.to_string(), model))
        })
        .collect();

    Some(Provider {
        id: provider_id.to_string(),
        name,
        env,
        npm,
        doc,
        api,
        models,
    })
}

/// Parse all providers from JSON body.
pub fn parse_all_providers(body: &str) -> Result<HashMap<String, Provider>, String> {
    let json: Value =
        serde_json::from_str(body).map_err(|e| format!("Failed to parse JSON: {}", e))?;

    let json_obj = json
        .as_object()
        .ok_or_else(|| "JSON is not an object".to_string())?;

    let mut providers = HashMap::new();

    for (provider_id, provider_value) in json_obj {
        if let Some(provider) = parse_provider(provider_id, provider_value) {
            providers.insert(provider_id.clone(), provider);
        }
    }

    Ok(providers)
}

/// Parse Model from JSON.
pub fn parse_model(model_id: &str, value: &Value) -> Option<Model> {
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(model_id)
        .to_string();

    let family = value
        .get("family")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let attachment = value
        .get("attachment")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let reasoning = value
        .get("reasoning")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let tool_call = value
        .get("tool_call")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let temperature = value
        .get("temperature")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let structured_output = value.get("structured_output").and_then(|v| v.as_bool());

    let knowledge = value
        .get("knowledge")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let release_date = value
        .get("release_date")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let last_updated = value
        .get("last_updated")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let modalities = value
        .get("modalities")
        .map(parse_modalities)
        .unwrap_or_default();

    let open_weights = value
        .get("open_weights")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let cost = value.get("cost").and_then(parse_cost);

    let limit = value.get("limit").and_then(parse_model_limit);

    Some(Model {
        id: model_id.to_string(),
        name,
        family,
        attachment,
        reasoning,
        tool_call,
        temperature,
        structured_output,
        knowledge,
        release_date,
        last_updated,
        modalities,
        open_weights,
        cost,
        limit,
    })
}

/// Parse ModelLimit from JSON.
pub fn parse_model_limit(limit: &Value) -> Option<ModelLimit> {
    let context = limit.get("context")?.as_u64()? as u32;
    let output = limit.get("output")?.as_u64()? as u32;

    let cache_read = limit
        .get("cache_read")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let cache_write = limit
        .get("cache_write")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    Some(ModelLimit {
        context,
        output,
        cache_read,
        cache_write,
    })
}

/// Extract provider api base from models.dev JSON.
pub fn extract_provider_api_from_models_dev_json(
    body: &str,
    provider_name: &str,
) -> Option<String> {
    let providers = parse_all_providers(body).ok()?;
    let provider = providers.get(provider_name).or_else(|| {
        providers
            .iter()
            .find(|(id, _)| id.eq_ignore_ascii_case(provider_name))
            .map(|(_, provider)| provider)
    })?;
    provider
        .api
        .as_deref()
        .map(str::trim)
        .filter(|api| !api.is_empty())
        .map(ToString::to_string)
}

fn parse_modalities(value: &Value) -> Modalities {
    let input = value
        .get("input")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str().and_then(|s| match s {
                        "text" => Some(ModalityType::Text),
                        "image" => Some(ModalityType::Image),
                        "audio" => Some(ModalityType::Audio),
                        "video" => Some(ModalityType::Video),
                        "pdf" => Some(ModalityType::Pdf),
                        _ => None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let output = value
        .get("output")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str().and_then(|s| match s {
                        "text" => Some(ModalityType::Text),
                        "image" => Some(ModalityType::Image),
                        "audio" => Some(ModalityType::Audio),
                        "video" => Some(ModalityType::Video),
                        "pdf" => Some(ModalityType::Pdf),
                        _ => None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Modalities { input, output }
}

fn parse_cost(value: &Value) -> Option<Cost> {
    let input = value
        .get("input")
        .and_then(|v| v.as_f64())
        .map(|v| (v * 100.0) as u32)?;

    let output = value
        .get("output")
        .and_then(|v| v.as_f64())
        .map(|v| (v * 100.0) as u32)?;

    let cache_read = value
        .get("cache_read")
        .and_then(|v| v.as_f64())
        .map(|v| (v * 100.0) as u32);

    let cache_write = value
        .get("cache_write")
        .and_then(|v| v.as_f64())
        .map(|v| (v * 100.0) as u32);

    Some(Cost {
        input,
        output,
        cache_read,
        cache_write,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_provider_api_reads_field() {
        let body = r#"{
            "openai": { "name": "OpenAI", "api": "https://api.openai.com/v1", "models": {} },
            "zhipuai-coding-plan": {
                "name": "BigModel",
                "api": "https://open.bigmodel.cn/api/paas/v4",
                "models": {}
            }
        }"#;
        let api = extract_provider_api_from_models_dev_json(body, "zhipuai-coding-plan");
        assert_eq!(api.as_deref(), Some("https://open.bigmodel.cn/api/paas/v4"));
    }

    #[test]
    fn extract_provider_api_matches_case_insensitive() {
        let body = r#"{
            "OpenAI": { "name": "OpenAI", "api": "https://api.openai.com/v1", "models": {} }
        }"#;
        let api = extract_provider_api_from_models_dev_json(body, "openai");
        assert_eq!(api.as_deref(), Some("https://api.openai.com/v1"));
    }
}
