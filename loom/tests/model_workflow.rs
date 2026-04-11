//! Tests for Model workflow refactoring.
//!
//! This test module verifies the new unified model configuration flow.

#[cfg(test)]
mod tests {
    use loom::llm::{create_llm_client, ModelEntry, ProviderConfig};

    /// Test 1: ModelEntry 可以正确创建 LLM client
    #[test]
    fn test_model_entry_creates_openai_client() {
        let entry = ModelEntry {
            id: "openai/gpt-4o".to_string(),
            name: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            api_key: Some("test-key".to_string()),
            provider_type: None,
            temperature: Some(0.7),
            max_tokens: Some(1000),
            tool_choice: None,
        };

        let result = create_llm_client(&entry);
        assert!(result.is_ok(), "Should create LLM client from ModelEntry");
    }

    /// Test 2: ModelEntry 可以创建 BigModel client
    #[test]
    fn test_model_entry_creates_bigmodel_client() {
        let entry = ModelEntry {
            id: "bigmodel/glm-4".to_string(),
            name: "glm-4".to_string(),
            provider: "bigmodel".to_string(),
            base_url: Some("https://open.bigmodel.cn/api/paas/v4".to_string()),
            api_key: Some("test-key".to_string()),
            provider_type: Some("bigmodel".to_string()),
            temperature: Some(0.7),
            max_tokens: Some(1000),
            tool_choice: None,
        };

        let result = create_llm_client(&entry);
        assert!(
            result.is_ok(),
            "Should create BigModel client from ModelEntry"
        );
    }

    /// Test 3: ModelEntry 缺少必需字段时返回错误
    #[test]
    fn test_model_entry_bigmodel_requires_base_url() {
        // Clear OPENAI_BASE_URL env var to ensure test doesn't pick up system config
        let prev_base_url = std::env::var("OPENAI_BASE_URL").ok();
        std::env::remove_var("OPENAI_BASE_URL");
        
        let entry = ModelEntry {
            id: "bigmodel/glm-4".to_string(),
            name: "glm-4".to_string(),
            provider: "bigmodel".to_string(),
            base_url: None, // Missing required field
            api_key: Some("test-key".to_string()),
            provider_type: Some("bigmodel".to_string()),
            temperature: None,
            max_tokens: None,
            tool_choice: None,
        };

        let result = create_llm_client(&entry);
        
        // Restore env var
        if let Some(v) = prev_base_url { std::env::set_var("OPENAI_BASE_URL", v) }
        
        assert!(
            result.is_err(),
            "Should fail when base_url is missing for bigmodel"
        );
    }

    /// Test 4: ProviderConfig 可以转换为 ModelEntry
    #[test]
    fn test_provider_config_to_model_entry() {
        let provider = ProviderConfig {
            name: "openai".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            api_key: Some("test-key".to_string()),
            provider_type: None,
        };

        let model = "gpt-4o";
        let entry = ModelEntry {
            id: format!("{}/{}", provider.name, model),
            name: model.to_string(),
            provider: provider.name.clone(),
            base_url: provider.base_url.clone(),
            api_key: provider.api_key.clone(),
            provider_type: provider.provider_type.clone(),
            temperature: None,
            max_tokens: None,
            tool_choice: None,
        };

        let result = create_llm_client(&entry);
        assert!(
            result.is_ok(),
            "Should create client from ProviderConfig-derived ModelEntry"
        );
    }

    /// Test 5: ModelEntry 默认值
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
}
