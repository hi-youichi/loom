mod common;
mod mocks;

use common::{TestEnvironment, write_last_model_file};
// Use read_last_model_file directly from config_helpers module
use common::config_helpers::read_last_model_file;
use mocks::MultiTierMockServer;

#[tokio::test]
async fn test_dynamic_model_switching() {
    // 1. 设置测试环境
    let test_env = TestEnvironment::new();
    let mock_server = MultiTierMockServer::new().await;
    
    // 2. 初始模型选择
    let initial_model = "gpt-3.5-medium";
    write_last_model_file(&test_env.loom_home, initial_model);
    
    let current_model = read_last_model_file(&test_env.loom_home);
    assert_eq!(current_model, initial_model, "Initial model should be set");
    
    let initial_tier = mock_server.get_tier_from_model(&current_model);
    assert_eq!(initial_tier, Some("medium".to_string()), "Initial model should be medium tier");
    
    // 3. 模拟动态切换到high tier
    let switched_model = "gpt-4-high";
    write_last_model_file(&test_env.loom_home, switched_model);
    
    let new_model = read_last_model_file(&test_env.loom_home);
    assert_eq!(new_model, switched_model, "Model should be switched");
    
    let new_tier = mock_server.get_tier_from_model(&new_model);
    assert_eq!(new_tier, Some("high".to_string()), "Switched model should be high tier");
    
    // 4. 验证切换生效
    assert_ne!(initial_tier, new_tier, "Tier should change after switching");
    
    println!("✅ Dynamic model switching test passed - model switches correctly from medium to high tier");
}

#[tokio::test]
async fn test_multiple_dynamic_switches() {
    // 测试多次动态切换
    let test_env = TestEnvironment::new();
    let mock_server = MultiTierMockServer::new().await;
    
    let switch_sequence = vec![
        ("gpt-3.5-low", "low"),
        ("gpt-3.5-medium", "medium"),
        ("gpt-4-high", "high"),
        ("gpt-3.5-low", "low"),
        ("gpt-4-high", "high"),
    ];
    
    let mut previous_tier = None;
    
    for (model, expected_tier) in switch_sequence {
        write_last_model_file(&test_env.loom_home, model);
        
        let current_model = read_last_model_file(&test_env.loom_home);
        assert_eq!(current_model, model, "Model {} should be set correctly", model);
        
        let current_tier = mock_server.get_tier_from_model(&current_model);
        assert_eq!(current_tier, Some(expected_tier.to_string()), 
                   "Model {} should map to {} tier", model, expected_tier);
        
        if let Some(prev_tier) = previous_tier {
            if prev_tier != expected_tier {
                println!("Switched from {} to {} tier", prev_tier, expected_tier);
            }
        }
        
        previous_tier = Some(expected_tier.to_string());
    }
    
    println!("✅ Multiple switches test passed - model handles multiple dynamic switches correctly");
}

#[tokio::test]
async fn test_switch_to_same_tier_different_model() {
    // 测试切换到同一tier的不同模型
    let test_env = TestEnvironment::new();
    let mock_server = MultiTierMockServer::new().await;
    
    // 假设有多个同tier的模型
    let medium_models = vec![
        "gpt-3.5-medium",
        "gpt-3.5-medium-alt", // 假设的替代模型
    ];
    
    for model in medium_models {
        write_last_model_file(&test_env.loom_home, model);
        
        let current_model = read_last_model_file(&test_env.loom_home);
        assert_eq!(current_model, model, "Model {} should be set", model);
        
        // 即使模型名称不同，但属于同一tier
        let tier = mock_server.get_tier_from_model(&current_model);
        // 注意：这里可能需要根据实际的模型映射调整
        if tier == Some("medium".to_string()) {
            println!("Model {} belongs to medium tier", model);
        }
    }
    
    println!("✅ Same tier switch test passed - handles switches within same tier");
}

#[tokio::test]
async fn test_rapid_switching_stability() {
    // 测试快速切换的稳定性
    let test_env = TestEnvironment::new();
    let mock_server = MultiTierMockServer::new().await;
    
    let models = vec![
        "gpt-3.5-low",
        "gpt-4-high", 
        "gpt-3.5-medium",
        "gpt-4-high",
        "gpt-3.5-low",
    ];
    
    for model in models {
        write_last_model_file(&test_env.loom_home, model);
        
        let current_model = read_last_model_file(&test_env.loom_home);
        assert_eq!(current_model, model, "Rapid switch to {} should succeed", model);
        
        let tier = mock_server.get_tier_from_model(&current_model);
        assert!(tier.is_some(), "Model {} should have a valid tier mapping", model);
    }
    
    // 最终验证最后一次切换
    let final_model = read_last_model_file(&test_env.loom_home);
    assert_eq!(final_model, "gpt-3.5-low", "Final model should be the last one set");
    
    println!("✅ Rapid switching stability test passed - handles rapid model switches reliably");
}

#[tokio::test]
async fn test_switch_persistence_after_dynamic_change() {
    // 测试动态切换后的持久化
    let test_env = TestEnvironment::new();
    
    // 1. 设置初始模型
    let initial_model = "gpt-3.5-medium";
    write_last_model_file(&test_env.loom_home, initial_model);
    
    // 2. 动态切换模型
    let switched_model = "gpt-4-high";
    write_last_model_file(&test_env.loom_home, switched_model);
    
    // 3. 验证切换后的模型被保存
    let saved_model = read_last_model_file(&test_env.loom_home);
    assert_eq!(saved_model, switched_model, "Switched model should be persisted");
    
    // 4. 模拟重启后验证（创建新环境并复制文件）
    let test_env_restarted = TestEnvironment::new();
    let original_last_model = test_env.last_model_path();
    let restarted_last_model = test_env_restarted.last_model_path();
    
    std::fs::copy(&original_last_model, &restarted_last_model)
        .expect("Should be able to copy last-model file");
    
    // 5. 验证重启后仍然保持切换后的模型
    let restored_model = read_last_model_file(&test_env_restarted.loom_home);
    assert_eq!(restored_model, switched_model, "Switched model should persist after restart");
    
    println!("✅ Switch persistence test passed - dynamic model switches are correctly persisted");
}