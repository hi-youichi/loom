use model_spec_core::*;

// ── Cost tests ──

#[test]
fn cost_new_basic() {
    let c = Cost::new(1.5, 3.0);
    assert_eq!(c.input, 1.5);
    assert_eq!(c.output, 3.0);
    assert_eq!(c.cache_read, None);
    assert_eq!(c.cache_write, None);
    assert_eq!(c.reasoning, None);
}

#[test]
fn cost_accessors() {
    let c = Cost::new(2.5, 5.0);
    assert_eq!(c.input_cost_usd(), 2.5);
    assert_eq!(c.output_cost_usd(), 5.0);
}

#[test]
fn cost_estimate_zero_tokens() {
    let c = Cost::new(10.0, 30.0);
    assert_eq!(c.estimate(0, 0), 0.0);
}

#[test]
fn cost_estimate_one_million_tokens() {
    let c = Cost::new(10.0, 30.0);
    let cost = c.estimate(1_000_000, 1_000_000);
    assert!((cost - 40.0).abs() < f64::EPSILON);
}

#[test]
fn cost_estimate_custom_tokens() {
    let c = Cost::new(1.0, 2.0);
    // 500K input * 1.0/1M = 0.5; 250K output * 2.0/1M = 0.5; total = 1.0
    let cost = c.estimate(500_000, 250_000);
    assert!((cost - 1.0).abs() < f64::EPSILON);
}

#[test]
fn cost_serde_roundtrip() {
    let c = Cost::new(1.5, 3.0);
    let json = serde_json::to_string(&c).unwrap();
    let de: Cost = serde_json::from_str(&json).unwrap();
    assert_eq!(c, de);
}

#[test]
fn cost_with_optional_fields_serde() {
    let json = r#"{"input":1.0,"output":2.0,"cache_read":0.5,"cache_write":0.25,"reasoning":3.0}"#;
    let c: Cost = serde_json::from_str(json).unwrap();
    assert_eq!(c.input, 1.0);
    assert_eq!(c.output, 2.0);
    assert_eq!(c.cache_read, Some(0.5));
    assert_eq!(c.cache_write, Some(0.25));
    assert_eq!(c.reasoning, Some(3.0));
}

#[test]
fn cost_defaults() {
    let json = r#"{}"#;
    let c: Cost = serde_json::from_str(json).unwrap();
    assert_eq!(c.input, 0.0);
    assert_eq!(c.output, 0.0);
    assert_eq!(c.cache_read, None);
}

// ── Limit tests ──

#[test]
fn model_limit_new() {
    let l = ModelLimit::new(128_000, 4096);
    assert_eq!(l.context, 128_000);
    assert_eq!(l.output, 4096);
    assert_eq!(l.cache_read, None);
    assert_eq!(l.cache_write, None);
}

#[test]
fn model_limit_builder_pattern() {
    let l = ModelLimit::new(200_000, 8192)
        .with_cache_read(200_000)
        .with_cache_write(50_000);
    assert_eq!(l.context, 200_000);
    assert_eq!(l.output, 8192);
    assert_eq!(l.cache_read, Some(200_000));
    assert_eq!(l.cache_write, Some(50_000));
}

#[test]
fn model_limit_serde_roundtrip() {
    let l = ModelLimit::new(100_000, 4096).with_cache_read(100_000);
    let json = serde_json::to_string(&l).unwrap();
    let de: ModelLimit = serde_json::from_str(&json).unwrap();
    assert_eq!(l, de);
}

// ── Modality tests ──

#[test]
fn modalities_default_empty() {
    let m = Modalities::default();
    assert!(m.input.is_empty());
    assert!(m.output.is_empty());
}

#[test]
fn modalities_supports_text() {
    let m = Modalities {
        input: vec![ModalityType::Text],
        output: vec![],
    };
    assert!(m.supports_text());
    assert!(!m.supports_vision());
    assert!(!m.supports_audio());
    assert!(!m.supports_video());
    assert!(!m.supports_pdf());
}

#[test]
fn modalities_supports_all() {
    let m = Modalities {
        input: vec![
            ModalityType::Text,
            ModalityType::Image,
            ModalityType::Audio,
            ModalityType::Video,
            ModalityType::Pdf,
        ],
        output: vec![ModalityType::Text],
    };
    assert!(m.supports_text());
    assert!(m.supports_vision());
    assert!(m.supports_audio());
    assert!(m.supports_video());
    assert!(m.supports_pdf());
}

#[test]
fn modality_type_serde() {
    let json = serde_json::to_string(&ModalityType::Text).unwrap();
    assert_eq!(json, "\"text\"");
    let json = serde_json::to_string(&ModalityType::Image).unwrap();
    assert_eq!(json, "\"image\"");
    let json = serde_json::to_string(&ModalityType::Audio).unwrap();
    assert_eq!(json, "\"audio\"");
    let json = serde_json::to_string(&ModalityType::Video).unwrap();
    assert_eq!(json, "\"video\"");
    let json = serde_json::to_string(&ModalityType::Pdf).unwrap();
    assert_eq!(json, "\"pdf\"");
}

// ── Model tests ──

#[test]
fn model_tier_derived() {
    let m = Model {
        id: "gpt-4o-mini".to_string(),
        name: "GPT-4o Mini".to_string(),
        family: Some("gpt-4o-mini".to_string()),
        attachment: false,
        reasoning: false,
        tool_call: true,
        temperature: true,
        structured_output: None,
        knowledge: None,
        release_date: None,
        last_updated: None,
        modalities: Modalities::default(),
        open_weights: false,
        cost: None,
        limit: None,
    };
    assert_eq!(m.tier(), ModelTier::Light);
}

#[test]
fn model_serde_roundtrip() {
    let m = Model {
        id: "test-model".to_string(),
        name: "Test Model".to_string(),
        family: None,
        attachment: true,
        reasoning: true,
        tool_call: true,
        temperature: true,
        structured_output: Some(true),
        knowledge: Some("2024-01".to_string()),
        release_date: Some("2024-01-01".to_string()),
        last_updated: Some("2024-06-01".to_string()),
        modalities: Modalities {
            input: vec![ModalityType::Text, ModalityType::Image],
            output: vec![ModalityType::Text],
        },
        open_weights: false,
        cost: Some(Cost::new(5.0, 15.0)),
        limit: Some(ModelLimit::new(128_000, 4096)),
    };
    let json = serde_json::to_string(&m).unwrap();
    let de: Model = serde_json::from_str(&json).unwrap();
    assert_eq!(m, de);
}

#[test]
fn model_defaults() {
    let json = r#"{"id":"x","name":"X"}"#;
    let m: Model = serde_json::from_str(json).unwrap();
    assert_eq!(m.id, "x");
    assert_eq!(m.name, "X");
    assert_eq!(m.family, None);
    assert!(!m.attachment);
    assert!(!m.reasoning);
    assert!(!m.tool_call);
    assert!(m.temperature); // default_true
    assert_eq!(m.structured_output, None);
    assert!(!m.open_weights);
}

// ── Provider tests ──

#[test]
fn provider_serde_roundtrip() {
    let p = Provider {
        id: "test-provider".to_string(),
        name: "Test Provider".to_string(),
        env: vec!["API_KEY".to_string()],
        npm: Some("test-sdk".to_string()),
        doc: Some("https://docs.example.com".to_string()),
        api: Some("https://api.example.com".to_string()),
        models: HashMap::new(),
    };
    let json = serde_json::to_string(&p).unwrap();
    let de: Provider = serde_json::from_str(&json).unwrap();
    assert_eq!(p, de);
}

use std::collections::HashMap;

// ── Parser tests ──

#[test]
fn parse_model_full() {
    let json = serde_json::json!({
        "name": "GPT-4o",
        "family": "gpt-4o",
        "attachment": true,
        "reasoning": false,
        "tool_call": true,
        "temperature": true,
        "cost": {"input": 5.0, "output": 15.0},
        "limit": {"context": 128000, "output": 4096}
    });
    let model = parse_model("gpt-4o", &json).unwrap();
    assert_eq!(model.id, "gpt-4o");
    assert_eq!(model.name, "GPT-4o");
    assert_eq!(model.family, Some("gpt-4o".to_string()));
    assert!(model.attachment);
    assert!(model.tool_call);
    assert!(model.cost.is_some());
    assert!(model.limit.is_some());
    assert_eq!(model.limit.unwrap().context, 128000);
}

#[test]
fn parse_model_minimal() {
    let json = serde_json::json!({});
    let model = parse_model("minimal", &json).unwrap();
    assert_eq!(model.id, "minimal");
    assert_eq!(model.name, "minimal"); // falls back to id
    assert_eq!(model.family, None);
    assert!(!model.attachment);
    assert!(!model.reasoning);
    assert!(!model.tool_call);
}

#[test]
fn parse_model_limit_with_cache() {
    let json = serde_json::json!({
        "context": 200000,
        "output": 8192,
        "cache_read": 200000,
        "cache_write": 50000
    });
    let limit = parse_model_limit(&json).unwrap();
    assert_eq!(limit.context, 200000);
    assert_eq!(limit.output, 8192);
    assert_eq!(limit.cache_read, Some(200000));
    assert_eq!(limit.cache_write, Some(50000));
}

#[test]
fn parse_model_limit_missing_context_returns_none() {
    let json = serde_json::json!({"output": 4096});
    assert!(parse_model_limit(&json).is_none());
}

#[test]
fn parse_provider_full() {
    let json = serde_json::json!({
        "name": "OpenAI",
        "env": ["OPENAI_API_KEY"],
        "api": "https://api.openai.com/v1",
        "models": {
            "gpt-4o": {
                "name": "GPT-4o",
                "cost": {"input": 5.0, "output": 15.0},
                "limit": {"context": 128000, "output": 4096}
            }
        }
    });
    let provider = parse_provider("openai", &json).unwrap();
    assert_eq!(provider.id, "openai");
    assert_eq!(provider.name, "OpenAI");
    assert_eq!(provider.env, vec!["OPENAI_API_KEY"]);
    assert_eq!(provider.api, Some("https://api.openai.com/v1".to_string()));
    assert_eq!(provider.models.len(), 1);
    assert!(provider.models.contains_key("gpt-4o"));
}

#[test]
fn parse_provider_minimal() {
    let json = serde_json::json!({"models": {}});
    let provider = parse_provider("minimal", &json).unwrap();
    assert_eq!(provider.id, "minimal");
    assert_eq!(provider.name, "minimal"); // falls back to provider_id
    assert!(provider.env.is_empty());
    assert!(provider.models.is_empty());
}

#[test]
fn parse_all_providers_valid() {
    let body = r#"{
        "openai": {"name": "OpenAI", "api": "https://api.openai.com/v1", "models": {}},
        "anthropic": {"name": "Anthropic", "api": "https://api.anthropic.com", "models": {}}
    }"#;
    let providers = parse_all_providers(body).unwrap();
    assert_eq!(providers.len(), 2);
    assert!(providers.contains_key("openai"));
    assert!(providers.contains_key("anthropic"));
}

#[test]
fn parse_all_providers_invalid_json() {
    let result = parse_all_providers("not json");
    assert!(result.is_err());
}

#[test]
fn parse_all_providers_not_object() {
    let result = parse_all_providers("[1,2,3]");
    assert!(result.is_err());
}

#[test]
fn extract_provider_api_empty_string_is_none() {
    let body = r#"{"prov": {"name": "P", "api": "  ", "models": {}}}"#;
    assert!(extract_provider_api_from_models_dev_json(body, "prov").is_none());
}

#[test]
fn extract_provider_api_no_api_field_is_none() {
    let body = r#"{"prov": {"name": "P", "models": {}}}"#;
    assert!(extract_provider_api_from_models_dev_json(body, "prov").is_none());
}

#[test]
fn extract_provider_api_not_found_is_none() {
    let body = r#"{"prov": {"name": "P", "api": "http://x", "models": {}}}"#;
    assert!(extract_provider_api_from_models_dev_json(body, "nonexistent").is_none());
}

#[test]
fn pick_best_for_tier_none_tier_returns_none() {
    let mut models = HashMap::new();
    models.insert(
        "model-1".to_string(),
        Model {
            id: "model-1".to_string(),
            name: "M1".to_string(),
            family: None,
            attachment: false,
            reasoning: false,
            tool_call: false,
            temperature: true,
            structured_output: None,
            knowledge: None,
            release_date: None,
            last_updated: None,
            modalities: Modalities::default(),
            open_weights: false,
            cost: None,
            limit: None,
        },
    );
    assert!(pick_best_for_tier(&models, ModelTier::None).is_none());
}

#[test]
fn pick_best_for_tier_empty_models_returns_none() {
    let models = HashMap::new();
    assert!(pick_best_for_tier(&models, ModelTier::Standard).is_none());
}

#[test]
fn pick_best_for_tier_picks_latest_release() {
    let mut models = HashMap::new();
    models.insert(
        "model-a".to_string(),
        Model {
            id: "model-a".to_string(),
            name: "A".to_string(),
            family: None,
            attachment: false,
            reasoning: false,
            tool_call: false,
            temperature: true,
            structured_output: None,
            knowledge: None,
            release_date: Some("2024-01-01".to_string()),
            last_updated: None,
            modalities: Modalities::default(),
            open_weights: false,
            cost: Some(Cost::new(3.0, 10.0)),
            limit: None,
        },
    );
    models.insert(
        "model-b".to_string(),
        Model {
            id: "model-b".to_string(),
            name: "B".to_string(),
            family: None,
            attachment: false,
            reasoning: false,
            tool_call: false,
            temperature: true,
            structured_output: None,
            knowledge: None,
            release_date: Some("2024-06-01".to_string()),
            last_updated: None,
            modalities: Modalities::default(),
            open_weights: false,
            cost: Some(Cost::new(3.0, 10.0)),
            limit: None,
        },
    );
    // Both are Standard tier by default
    let result = pick_best_for_tier(&models, ModelTier::Standard);
    assert!(result.is_some());
    let (id, _) = result.unwrap();
    assert_eq!(id, "model-b"); // newer release date
}

#[test]
fn model_tier_variants() {
    let variants = ModelTier::variants();
    assert_eq!(variants, ["none", "light", "standard", "strong"]);
}

#[test]
fn model_tier_display() {
    assert_eq!(ModelTier::None.to_string(), "none");
    assert_eq!(ModelTier::Light.to_string(), "light");
    assert_eq!(ModelTier::Standard.to_string(), "standard");
    assert_eq!(ModelTier::Strong.to_string(), "strong");
}

#[test]
fn model_tier_default() {
    assert_eq!(ModelTier::default(), ModelTier::None);
}

#[test]
fn parse_model_with_modalities() {
    let json = serde_json::json!({
        "name": "Multi",
        "modalities": {
            "input": ["text", "image", "audio"],
            "output": ["text"]
        }
    });
    let model = parse_model("multi", &json).unwrap();
    assert!(model.modalities.supports_text());
    assert!(model.modalities.supports_vision());
    assert!(model.modalities.supports_audio());
    assert!(!model.modalities.supports_video());
}

#[test]
fn parse_model_with_open_weights() {
    let json = serde_json::json!({"name": "Open", "open_weights": true});
    let model = parse_model("open", &json).unwrap();
    assert!(model.open_weights);
}

#[test]
fn parse_model_with_reasoning() {
    let json = serde_json::json!({"name": "R1", "reasoning": true});
    let model = parse_model("r1", &json).unwrap();
    assert!(model.reasoning);
}

#[test]
fn cost_equality() {
    let a = Cost::new(1.0, 2.0);
    let b = Cost::new(1.0, 2.0);
    assert_eq!(a, b);
}

#[test]
fn modality_type_hash_and_equality() {
    use std::collections::HashSet;
    let set: HashSet<ModalityType> = vec![ModalityType::Text, ModalityType::Text].into_iter().collect();
    assert_eq!(set.len(), 1);
}
