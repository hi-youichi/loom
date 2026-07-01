//! Tests for InvokeAgentTool.

#![allow(dead_code, unused_imports)]

use std::sync::Arc;

use super::*;
use crate::tools::invoke_agent::build_config::build_config_from_profile;
use crate::profile::resolve_profile;

fn make_tool() -> InvokeAgentTool {
    InvokeAgentTool::new(Arc::new(ReactBuildConfig::from_env()), Some(3))
}

#[tokio::test(flavor = "current_thread")]
async fn depth_exceeded_returns_error() {
    let tool = InvokeAgentTool::new(Arc::new(ReactBuildConfig::from_env()), Some(2));
    let args = serde_json::json!({
        "agents": [{"agent": "dev", "task": "hello"}]
    });
    let ctx = ToolCallContext {
        depth: 2,
        ..Default::default()
    };
    let result = tool.call(args, Some(&ctx)).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("max sub-agent depth"), "error: {}", err);
}

#[tokio::test(flavor = "current_thread")]
async fn missing_agents_arg_returns_error() {
    let tool = make_tool();
    let args = serde_json::json!({"fail_fast": false});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("agents"), "error: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn empty_agents_array_returns_error() {
    let tool = make_tool();
    let args = serde_json::json!({"agents": []});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("empty"));
}

#[tokio::test(flavor = "current_thread")]
async fn missing_task_in_single_item_returns_error() {
    let tool = make_tool();
    let args = serde_json::json!({"agents": [{"agent": "dev"}]});
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("task"));
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_agent_returns_error() {
    let tool = make_tool();
    let args = serde_json::json!({
        "agents": [{"agent": "nonexistent-xyz", "task": "hello"}]
    });
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("nonexistent-xyz"));
}

#[tokio::test(flavor = "current_thread")]
async fn batch_call_missing_agent_in_array_returns_error() {
    let tool = make_tool();
    let args = serde_json::json!({
        "agents": [
            {"task": "hello"}
        ]
    });
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("agent"));
}

#[tokio::test(flavor = "current_thread")]
async fn batch_call_missing_task_in_array_returns_error() {
    let tool = make_tool();
    let args = serde_json::json!({
        "agents": [
            {"agent": "dev"}
        ]
    });
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("task"));
}

#[tokio::test(flavor = "current_thread")]
async fn batch_call_with_invalid_agents_array_returns_error() {
    let tool = make_tool();
    let args = serde_json::json!({
        "agents": "not-an-array"
    });
    let result = tool.call(args, None).await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("agents") && msg.contains("array"),
        "error: {}",
        msg
    );
}

struct MockTierResolver {
    light_model_id: String,
}

#[async_trait::async_trait]
impl model_spec_core::TierResolver for MockTierResolver {
    async fn resolve_tier(
        &self,
        _model: Option<&str>,
        tier: model_spec_core::ModelTier,
        _provider_hint: Option<&str>,
        _providers: &[model_spec_core::ProviderConfig],
    ) -> Option<model_spec_core::ResolvedTierModel> {
        assert_eq!(
            tier,
            model_spec_core::ModelTier::Light,
            "explore agent should request Light tier"
        );
        Some(model_spec_core::ResolvedTierModel {
            model_id: self.light_model_id.clone(),
            base_url: Some("https://mock.test/v1".into()),
            api_key: Some("sk-mock".into()),
            provider_type: Some("openai_compat".into()),
            provider_name: Some("mock-provider".into()),
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn explore_agent_resolves_light_tier_model() {
    let profile = resolve_profile("explore").expect("explore profile should load");
    assert_eq!(
        profile.model.as_ref().and_then(|m| m.tier),
        Some(model_spec_core::ModelTier::Light),
        "explore agent config.yaml should declare tier: light"
    );

    let mut parent_config = ReactBuildConfig::from_env();
    parent_config.model = None;
    parent_config.openai_base_url = None;
    parent_config.openai_api_key = None;
    parent_config.llm_provider = None;
    let sub_config = build_config_from_profile(&profile, &parent_config, None);
    assert_eq!(
        sub_config.model_tier,
        Some(model_spec_core::ModelTier::Light),
        "build_config_from_profile should propagate explore's light tier"
    );

    let resolver = MockTierResolver {
        light_model_id: "anthropic/claude-haiku-4".to_string(),
    };
    let resolved =
        crate::agent::react::tier_apply::resolve_tier_and_build_config_with_resolver(&sub_config, &resolver).await;

    assert_eq!(
        resolved.model.as_deref(),
        Some("anthropic/claude-haiku-4"),
        "resolved model should be the light-tier model from MockTierResolver"
    );
    assert!(
        resolved.model_tier.is_none(),
        "model_tier should be cleared after resolution"
    );
    assert_eq!(
        resolved.openai_base_url.as_deref(),
        Some("https://mock.test/v1"),
        "base_url should come from resolved tier model"
    );
    assert_eq!(
        resolved.openai_api_key.as_deref(),
        Some("sk-mock"),
        "api_key should come from resolved tier model"
    );
    assert_eq!(
        resolved.llm_provider.as_deref(),
        Some("openai_compat"),
        "provider_type should come from resolved tier model"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn explore_agent_config_inheritance_with_parent_tier() {
    let profile = resolve_profile("explore").expect("explore profile should load");

    let mut parent_config = ReactBuildConfig::from_env();
    parent_config.model_tier = Some(model_spec_core::ModelTier::Strong);

    let sub_config = build_config_from_profile(&profile, &parent_config, None);

    assert_eq!(
        sub_config.model_tier,
        Some(model_spec_core::ModelTier::Light),
        "explore's tier: light should override parent's tier"
    );
}
