//! Tests for Model workflow refactoring.
//!
//! This test file verifies the unified model configuration flow:
//! ModelEntry → create_llm_client → LlmClient

#[cfg(test)]
mod tests {
    use loom::llm::{create_llm_client, ModelEntry, ModelRegistry, ProviderConfig, ToolChoiceMode};

    /// Test ModelEntry default values
    #[test]
    fn test_model_entry_default() {
        let entry = ModelEntry::default();
        assert!(entry.id.is_empty());
        assert!(entry.name.is_empty());
        assert!(entry.provider.is_empty());
        assert!(entry.base_url.is_none());
        assert!(entry.api_key.is_none());
        assert!(entry.provider_type.is_none());
        assert!(entry.temperature.is_none());
        assert!(entry.max_tokens.is_none());
        assert!(entry.tool_choice.is_none());
    }

    /// Test ModelEntry builder methods
    #[test]
    fn test_model_entry_builder() {
        let entry = ModelEntry {
            id: "openai/gpt-4o".to_string(),
            name: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            api_key: Some("sk-test".to_string()),
            provider_type: Some("openai".to_string()),
            temperature: Some(0.7),
            max_tokens: Some(1000),
            tool_choice: Some(ToolChoiceMode::Auto),
        };

        assert_eq!(entry.id, "openai/gpt-4o");
        assert_eq!(entry.name, "gpt-4o");
        assert_eq!(entry.provider, "openai");
        assert_eq!(entry.temperature, Some(0.7));
        assert_eq!(entry.max_tokens, Some(1000));
    }

    /// Test create_llm_client with OpenAI-compatible provider
    #[test]
    fn test_create_llm_client_openai() {
        let entry = ModelEntry {
            id: "openai/gpt-4o".to_string(),
            name: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            api_key: Some("sk-test-key".to_string()),
            provider_type: None, // Default to OpenAI
            ..Default::default()
        };

        let result = create_llm_client(&entry);
        assert!(result.is_ok());
    }

    /// Test create_llm_client with BigModel provider
    #[test]
    fn test_create_llm_client_bigmodel() {
        let entry = ModelEntry {
            id: "bigmodel/glm-4".to_string(),
            name: "glm-4".to_string(),
            provider: "bigmodel".to_string(),
            base_url: Some("https://open.bigmodel.cn/api/paas/v4".to_string()),
            api_key: Some("test-api-key".to_string()),
            provider_type: Some("bigmodel".to_string()),
            ..Default::default()
        };

        let result = create_llm_client(&entry);
        assert!(result.is_ok());
    }

    /// Test create_llm_client with missing api_key for BigModel (should fail)
    #[test]
    fn test_create_llm_client_bigmodel_missing_api_key() {
        let entry = ModelEntry {
            id: "bigmodel/glm-4".to_string(),
            name: "glm-4".to_string(),
            provider: "bigmodel".to_string(),
            base_url: Some("https://open.bigmodel.cn/api/paas/v4".to_string()),
            api_key: None, // Missing!
            provider_type: Some("bigmodel".to_string()),
            ..Default::default()
        };

        let result = create_llm_client(&entry);
        assert!(result.is_err());
    }

    /// Test create_llm_client with missing base_url for BigModel (should fail)
    #[test]
    fn test_create_llm_client_bigmodel_missing_base_url() {
        // Clear OPENAI_BASE_URL env var to ensure test doesn't pick up system config
        let prev_base_url = std::env::var("OPENAI_BASE_URL").ok();
        std::env::remove_var("OPENAI_BASE_URL");

        let entry = ModelEntry {
            id: "bigmodel/glm-4".to_string(),
            name: "glm-4".to_string(),
            provider: "bigmodel".to_string(),
            base_url: None, // Missing!
            api_key: Some("test-api-key".to_string()),
            provider_type: Some("bigmodel".to_string()),
            ..Default::default()
        };

        let result = create_llm_client(&entry);

        // Restore env var
        if let Some(v) = prev_base_url {
            std::env::set_var("OPENAI_BASE_URL", v)
        }

        assert!(result.is_err());
    }

    /// Test ProviderConfig to ModelEntry conversion
    #[test]
    fn test_provider_config_to_model_entry() {
        let provider = ProviderConfig {
            name: "openai".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            api_key: Some("sk-test".to_string()),
            provider_type: Some("openai".to_string()),
            fetch_models: false,
            cache_ttl: None,
            enable_tier_resolution: true,
        };

        let model_name = "gpt-4o";
        let entry = ModelEntry {
            id: format!("{}/{}", provider.name, model_name),
            name: model_name.to_string(),
            provider: provider.name.clone(),
            base_url: provider.base_url.clone(),
            api_key: provider.api_key.clone(),
            provider_type: provider.provider_type.clone(),
            ..Default::default()
        };

        assert_eq!(entry.id, "openai/gpt-4o");
        assert_eq!(entry.name, "gpt-4o");
        assert_eq!(entry.provider, "openai");
    }

    /// Test ModelRegistry list_all_models with empty providers
    #[tokio::test]
    async fn test_model_registry_list_empty_providers() {
        let registry = ModelRegistry::new();
        let models = registry.list_all_models(&[]).await;
        assert!(models.is_empty());
    }

    /// Test run_agent_with_provider exists and compiles
    #[test]
    fn test_run_agent_with_provider_signature() {
        // This test just verifies the function exists with the correct signature
        // Actual async testing would require a runtime
        use loom::cli_run::run_agent_with_provider;

        // Verify the function exists
        let _fn_ptr: fn(_, _, _, _) -> _ = run_agent_with_provider;
    }
}
