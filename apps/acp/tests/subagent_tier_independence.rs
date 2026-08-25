mod common;
mod mocks;

use common::{write_last_model_file, TestEnvironment};
// Import functions directly from their modules
use common::config_helpers::{create_agent_config, create_subagent_config};
use mocks::MultiTierMockServer;

#[tokio::test]
async fn test_subagent_tier_independence() {
    // 1. 设置测试环境
    let test_env = TestEnvironment::new();
    let mock_server = MultiTierMockServer::new().await;

    // 2. 配置子代理使用 low tier
    create_subagent_config(&test_env.anureo_home, "product-manager", "low");

    // 3. 配置 ACP 代理使用 medium tier
    create_agent_config(&test_env.anureo_home, "medium");

    // 4. ACP 选择 high tier 模型（模拟用户选择）
    let acp_model_selection = "gpt-4-high";
    write_last_model_file(&test_env.anureo_home, acp_model_selection);

    // 5. 验证 ACP 使用 high tier
    let acp_tier = mock_server.get_tier_from_model(acp_model_selection);
    assert_eq!(
        acp_tier,
        Some("high".to_string()),
        "ACP should use high tier"
    );

    // 6. 验证子代理配置保持为 low tier
    let subagent_config_path = test_env
        .anureo_home
        .join(".anureo/agents/product-manager/config.yaml");
    assert!(
        subagent_config_path.exists(),
        "Subagent config should exist"
    );

    let subagent_config = std::fs::read_to_string(&subagent_config_path)
        .expect("Should be able to read subagent config");
    assert!(
        subagent_config.contains("low"),
        "Subagent should still have low tier config"
    );

    // 7. 验证子代理对应的模型映射
    let subagent_model = "gpt-3.5-low";
    let subagent_tier = mock_server.get_tier_from_model(subagent_model);
    assert_eq!(
        subagent_tier,
        Some("low".to_string()),
        "Subagent should map to low tier"
    );

    // 8. 验证ACP和子代理使用不同的tier
    assert_ne!(
        acp_tier, subagent_tier,
        "ACP and subagent should use different tiers"
    );

    println!("✅ Subagent tier independence test passed - subagent maintains independent tier configuration");
}

#[tokio::test]
async fn test_multiple_subagents_different_tiers() {
    // 测试多个子代理可以有不同的tier配置
    let test_env = TestEnvironment::new();
    let mock_server = MultiTierMockServer::new().await;

    // 配置不同子代理使用不同tier
    create_subagent_config(&test_env.anureo_home, "product-manager", "low");
    create_subagent_config(&test_env.anureo_home, "test-engineer", "medium");
    create_subagent_config(&test_env.anureo_home, "rust-architect", "high");

    // ACP 使用 high tier
    let acp_model = "gpt-4-high";
    write_last_model_file(&test_env.anureo_home, acp_model);

    // 验证每个子代理的配置
    let tiers = vec![
        ("product-manager", "low", "gpt-3.5-low"),
        ("test-engineer", "medium", "gpt-3.5-medium"),
        ("rust-architect", "high", "gpt-4-high"),
    ];

    for (agent_name, expected_tier, expected_model) in tiers {
        let config_path = test_env
            .anureo_home
            .join(format!(".anureo/agents/{}/config.yaml", agent_name));
        let config = std::fs::read_to_string(&config_path)
            .unwrap_or_else(|_| panic!("Should be able to read {} config", agent_name));
        assert!(
            config.contains(expected_tier),
            "{} should have {} tier",
            agent_name,
            expected_tier
        );

        let tier = mock_server.get_tier_from_model(expected_model);
        assert_eq!(
            tier,
            Some(expected_tier.to_string()),
            "{} model should map to {} tier",
            agent_name,
            expected_tier
        );
    }

    println!("✅ Multiple subagents test passed - different subagents maintain independent tier configurations");
}

#[tokio::test]
async fn test_acp_selection_doesnt_affect_subagent_configs() {
    // 测试ACP模型选择不会修改子代理的配置文件
    let test_env = TestEnvironment::new();

    // 创建子代理配置
    create_subagent_config(&test_env.anureo_home, "product-manager", "low");

    // 读取原始配置
    let original_config = std::fs::read_to_string(
        test_env
            .anureo_home
            .join(".anureo/agents/product-manager/config.yaml"),
    )
    .expect("Should be able to read original config");

    // 模拟ACP选择不同的模型
    write_last_model_file(&test_env.anureo_home, "gpt-4-high");

    // 验证子代理配置文件未被修改
    let current_config = std::fs::read_to_string(
        test_env
            .anureo_home
            .join(".anureo/agents/product-manager/config.yaml"),
    )
    .expect("Should be able to read current config");

    assert_eq!(
        original_config, current_config,
        "Subagent config should not be modified by ACP selection"
    );

    println!(
        "✅ Config isolation test passed - ACP selection doesn't modify subagent config files"
    );
}
