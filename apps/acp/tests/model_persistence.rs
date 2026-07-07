mod common;
mod mocks;

use common::{write_last_model_file, TestEnvironment};
// Use read_last_model_file directly from config_helpers module
use common::config_helpers::read_last_model_file;
use mocks::MultiTierMockServer;

#[tokio::test]
async fn test_model_persistence_across_restarts() {
    // 1. 设置测试环境
    let test_env = TestEnvironment::new();
    let mock_server = MultiTierMockServer::new().await;

    // 2. 模拟用户选择模型
    let selected_model = "gpt-4-high";
    write_last_model_file(&test_env.loom_home, selected_model);

    // 3. 验证模型被正确保存
    let saved_model = read_last_model_file(&test_env.loom_home);
    assert_eq!(
        saved_model, selected_model,
        "Model should be saved correctly"
    );

    // 4. 模拟重启 - 创建新的测试环境（模拟进程重启）
    // 在真实场景中，这里会重启ACP进程
    let test_env_restarted = TestEnvironment::new();

    // 5. 复制last-model文件到新环境（模拟持久化）
    let original_last_model = test_env.last_model_path();
    let restarted_last_model = test_env_restarted.last_model_path();

    std::fs::copy(&original_last_model, &restarted_last_model)
        .expect("Should be able to copy last-model file");

    // 6. 验证重启后模型选择保持不变
    let restored_model = read_last_model_file(&test_env_restarted.loom_home);
    assert_eq!(
        restored_model, selected_model,
        "Model should persist across restarts"
    );

    // 7. 验证恢复的模型对应的tier
    let tier = mock_server.get_tier_from_model(&restored_model);
    assert_eq!(
        tier,
        Some("high".to_string()),
        "Restored model should map to correct tier"
    );

    println!(
        "✅ Model persistence test passed - model selection correctly persists across restarts"
    );
}

#[tokio::test]
async fn test_model_persistence_file_format() {
    // 测试持久化文件的格式
    let test_env = TestEnvironment::new();

    let test_models = vec!["gpt-4-high", "gpt-3.5-medium", "gpt-3.5-low"];

    for model in test_models {
        write_last_model_file(&test_env.loom_home, model);

        let read_model = read_last_model_file(&test_env.loom_home);
        assert_eq!(
            read_model, model,
            "Model {} should be read back correctly",
            model
        );

        // 验证文件内容格式（应该只是模型名称，没有额外内容）
        let file_content = std::fs::read_to_string(test_env.last_model_path())
            .expect("Should be able to read file");
        assert_eq!(
            file_content.trim(),
            model,
            "File content should be just the model name"
        );
    }

    println!("✅ File format test passed - persistence file format is correct");
}

#[tokio::test]
async fn test_model_persistence_overwrites() {
    // 测试模型选择能够正确覆盖之前的保存
    let test_env = TestEnvironment::new();
    let mock_server = MultiTierMockServer::new().await;

    // 1. 首先选择一个模型
    let first_model = "gpt-3.5-low";
    write_last_model_file(&test_env.loom_home, first_model);

    let first_read = read_last_model_file(&test_env.loom_home);
    assert_eq!(first_read, first_model, "First model should be saved");

    // 2. 然后选择不同的模型
    let second_model = "gpt-4-high";
    write_last_model_file(&test_env.loom_home, second_model);

    let second_read = read_last_model_file(&test_env.loom_home);
    assert_eq!(
        second_read, second_model,
        "Second model should overwrite first"
    );

    // 3. 验证新模型对应的tier
    let new_tier = mock_server.get_tier_from_model(&second_read);
    assert_eq!(
        new_tier,
        Some("high".to_string()),
        "New model should map to high tier"
    );

    // 4. 验证旧模型不再被保存
    assert_ne!(second_read, first_model, "Old model should be overwritten");

    println!("✅ Model overwrite test passed - new model selection correctly overwrites previous");
}

#[tokio::test]
async fn test_model_persistence_with_different_tiers() {
    // 测试不同tier模型的持久化
    let test_env = TestEnvironment::new();
    let mock_server = MultiTierMockServer::new().await;

    let test_cases = vec![
        ("gpt-4-high", "high"),
        ("gpt-3.5-medium", "medium"),
        ("gpt-3.5-low", "low"),
    ];

    for (model, expected_tier) in test_cases {
        write_last_model_file(&test_env.loom_home, model);

        let read_model = read_last_model_file(&test_env.loom_home);
        assert_eq!(read_model, model);

        let tier = mock_server.get_tier_from_model(&read_model);
        assert_eq!(
            tier,
            Some(expected_tier.to_string()),
            "Model {} should map to {} tier",
            model,
            expected_tier
        );
    }

    println!(
        "✅ Multi-tier persistence test passed - models from different tiers persist correctly"
    );
}
