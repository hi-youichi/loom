use std::collections::HashMap;

use super::provider::Provider;
use super::yaml_provider::YamlPluginFile;

/// Compile-time embedded bundled provider YAML files.
///
/// Bundled providers are embedded at compile time via `include_str!`
/// and serve as the highest-priority data source (not user-overridable).
const BUNDLED_PROVIDER_YAMLS: &[&str] = &[include_str!(
    "../../bundled-providers/huoshan-coding-plan.yaml"
)];

/// Load all bundled providers embedded at compile time.
///
/// Returns a map of normalized provider name → Provider with its models populated.
pub fn load_bundled_providers() -> HashMap<String, Provider> {
    let mut providers: HashMap<String, Provider> = HashMap::new();

    for yaml_text in BUNDLED_PROVIDER_YAMLS {
        let Ok(plugin) = serde_yaml::from_str::<YamlPluginFile>(yaml_text) else {
            continue;
        };
        let (provider, models) = plugin.into_provider_and_models();
        let key = normalize_provider_name(&provider.id);
        let provider = Provider {
            models, // populate models into the provider
            ..provider
        };
        providers.insert(key, provider);
    }

    providers
}

/// Normalize a provider name to lowercase, trimmed.
fn normalize_provider_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_bundled_providers() {
        let providers = load_bundled_providers();
        assert!(
            !providers.is_empty(),
            "should have at least one bundled provider"
        );

        let huoshan = providers.get("huoshan-coding-plan");
        assert!(huoshan.is_some(), "huoshan-coding-plan should be bundled");

        let provider = huoshan.unwrap();
        assert_eq!(provider.id, "huoshan-coding-plan");
        assert_eq!(provider.name, "Huoshan Coding Plan");
        assert_eq!(
            provider.api.as_deref(),
            Some("https://ark.cn-beijing.volces.com/api/coding/v3")
        );
        assert!(!provider.models.is_empty(), "should have models");

        let model = provider.models.get("doubao-seed-2-1-pro-260628");
        assert!(model.is_some(), "doubao model should exist");
        assert_eq!(model.unwrap().limit.context, 262144);
    }
}
