use std::collections::HashMap;

use serde_json::Value;

use super::cost::{Cost, CostTier, CostTierInfo};
use super::limit::{Modalities, ModalityType, ModelLimit};
use super::model::{
    Experimental, ExperimentalMode, ExperimentalProviderConfig, Interleaved,
    InterleavedField, Model, ModelProviderConfig, ModelStatus, ProviderShape,
    ReasoningEffort, ReasoningOption,
};
use super::provider::Provider;

/// Parse Provider from JSON.
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

    let description = value
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
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

    let reasoning_options = value
        .get("reasoning_options")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_reasoning_option).collect());

    let tool_call = value
        .get("tool_call")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let interleaved = value.get("interleaved").and_then(parse_interleaved);

    let structured_output = value.get("structured_output").and_then(|v| v.as_bool());

    let temperature = value.get("temperature").and_then(|v| v.as_bool());

    let knowledge = value
        .get("knowledge")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let release_date = value
        .get("release_date")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let last_updated = value
        .get("last_updated")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let modalities = value
        .get("modalities")
        .map(parse_modalities)
        .unwrap_or_default();

    let open_weights = value
        .get("open_weights")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let cost = value.get("cost").and_then(parse_cost);

    let limit = value.get("limit").and_then(parse_model_limit)?;

    let status = value
        .get("status")
        .and_then(|v| v.as_str())
        .and_then(parse_model_status);

    let experimental = value.get("experimental").and_then(parse_experimental);

    let provider = value
        .get("provider")
        .and_then(parse_model_provider_config);

    Some(Model {
        id: model_id.to_string(),
        name,
        description,
        family,
        attachment,
        reasoning,
        reasoning_options,
        tool_call,
        interleaved,
        structured_output,
        temperature,
        knowledge,
        release_date,
        last_updated,
        modalities,
        open_weights,
        cost,
        limit,
        status,
        experimental,
        provider,
    })
}

/// Parse ModelLimit from JSON.
pub fn parse_model_limit(limit: &Value) -> Option<ModelLimit> {
    let context = limit.get("context")?.as_u64()? as u32;
    let output = limit.get("output")?.as_u64()? as u32;

    let input = limit
        .get("input")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    Some(ModelLimit {
        context,
        input,
        output,
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

// ── Private parsing helpers ──

fn parse_modalities(value: &Value) -> Modalities {
    let input = value
        .get("input")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_modality_type).collect())
        .unwrap_or_default();

    let output = value
        .get("output")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_modality_type).collect())
        .unwrap_or_default();

    Modalities { input, output }
}

fn parse_modality_type(v: &Value) -> Option<ModalityType> {
    v.as_str().and_then(|s| match s {
        "text" => Some(ModalityType::Text),
        "image" => Some(ModalityType::Image),
        "audio" => Some(ModalityType::Audio),
        "video" => Some(ModalityType::Video),
        "pdf" => Some(ModalityType::Pdf),
        _ => None,
    })
}

fn parse_cost(value: &Value) -> Option<Cost> {
    let input = value.get("input").and_then(|v| v.as_f64())?;
    let output = value.get("output").and_then(|v| v.as_f64())?;

    let reasoning = value.get("reasoning").and_then(|v| v.as_f64());
    let cache_read = value.get("cache_read").and_then(|v| v.as_f64());
    let cache_write = value.get("cache_write").and_then(|v| v.as_f64());
    let input_audio = value.get("input_audio").and_then(|v| v.as_f64());
    let output_audio = value.get("output_audio").and_then(|v| v.as_f64());

    let context_over_200k = value
        .get("context_over_200k")
        .and_then(parse_cost)
        .map(Box::new);

    let tiers = value
        .get("tiers")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_cost_tier).collect());

    Some(Cost {
        input,
        output,
        reasoning,
        cache_read,
        cache_write,
        input_audio,
        output_audio,
        context_over_200k,
        tiers,
    })
}

fn parse_cost_tier(value: &Value) -> Option<CostTier> {
    let cost = parse_cost(value)?;
    let tier = value.get("tier").and_then(|t| {
        let size = t.get("size")?.as_u64()?;
        Some(CostTierInfo {
            r#type: t
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("context")
                .to_string(),
            size,
        })
    });
    Some(CostTier { cost, tier })
}

fn parse_reasoning_option(v: &Value) -> Option<ReasoningOption> {
    let typ = v.get("type")?.as_str()?;
    match typ {
        "toggle" => Some(ReasoningOption::Toggle),
        "effort" => {
            let values = v
                .get("values")
                .and_then(|v| v.as_array())?
                .iter()
                .map(parse_reasoning_effort)
                .collect();
            Some(ReasoningOption::Effort { values })
        }
        "budget_tokens" => {
            let min = v.get("min").and_then(|v| v.as_f64());
            let max = v.get("max").and_then(|v| v.as_f64());
            Some(ReasoningOption::BudgetTokens { min, max })
        }
        _ => None,
    }
}

fn parse_reasoning_effort(v: &Value) -> Option<ReasoningEffort> {
    if v.is_null() {
        return None;
    }
    v.as_str().and_then(|s| match s {
        "none" => Some(ReasoningEffort::None),
        "minimal" => Some(ReasoningEffort::Minimal),
        "low" => Some(ReasoningEffort::Low),
        "medium" => Some(ReasoningEffort::Medium),
        "high" => Some(ReasoningEffort::High),
        "xhigh" => Some(ReasoningEffort::Xhigh),
        "max" => Some(ReasoningEffort::Max),
        "default" => Some(ReasoningEffort::Default),
        _ => None,
    })
}

fn parse_interleaved(v: &Value) -> Option<Interleaved> {
    if v.as_bool() == Some(true) {
        return Some(Interleaved::Simple);
    }
    let field = v
        .get("field")
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "reasoning_content" => Some(InterleavedField::ReasoningContent),
            "reasoning_details" => Some(InterleavedField::ReasoningDetails),
            _ => None,
        })?;
    Some(Interleaved::Field { field })
}

fn parse_model_status(s: &str) -> Option<ModelStatus> {
    match s {
        "alpha" => Some(ModelStatus::Alpha),
        "beta" => Some(ModelStatus::Beta),
        "deprecated" => Some(ModelStatus::Deprecated),
        _ => None,
    }
}

fn parse_experimental(v: &Value) -> Option<Experimental> {
    let modes = v
        .get("modes")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(key, mode_val)| {
                    parse_experimental_mode(mode_val).map(|m| (key.clone(), m))
                })
                .collect()
        });
    Some(Experimental { modes })
}

fn parse_experimental_mode(v: &Value) -> Option<ExperimentalMode> {
    let cost = v.get("cost").and_then(parse_cost);
    let provider = v
        .get("provider")
        .and_then(parse_experimental_provider_config);
    Some(ExperimentalMode { cost, provider })
}

fn parse_experimental_provider_config(v: &Value) -> Option<ExperimentalProviderConfig> {
    let body = v
        .get("body")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    let headers = v
        .get("headers")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        });
    Some(ExperimentalProviderConfig { body, headers })
}

fn parse_model_provider_config(v: &Value) -> Option<ModelProviderConfig> {
    let npm = v.get("npm").and_then(|v| v.as_str()).map(|s| s.to_string());
    let api = v.get("api").and_then(|v| v.as_str()).map(|s| s.to_string());
    let shape = v
        .get("shape")
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "responses" => Some(ProviderShape::Responses),
            "completions" => Some(ProviderShape::Completions),
            _ => None,
        });
    let body = v
        .get("body")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    let headers = v
        .get("headers")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        });
    Some(ModelProviderConfig {
        npm,
        api,
        shape,
        body,
        headers,
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
