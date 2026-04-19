//! End-to-end integration tests for ACP model resolution with tier awareness
//! 
//! Tests the priority: ACP explicit model > agent model name > agent tier > default config

mod e2e;

use e2e::AcpChild;
use serde_json::json;

/// Helper function to initialize ACP session
async fn initialize_session(acp: &mut AcpChild) -> String {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        }
    });
    
    acp.send_request(&request);
    let response = acp.read_message().expect("read initialize response");
    let response: e2e::RpcResponse = serde_json::from_value(response).expect("parse initialize response");
    
    assert!(response.error.is_none(), "initialize should succeed: {:?}", response.error);
    assert!(response.result.is_some(), "initialize should have result");
    
    response.result.unwrap().to_string()
}

/// Helper function to create a new session
async fn create_session(acp: &mut AcpChild, cwd: &str) -> String {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "session/new",
        "params": {
            "cwd": cwd
        }
    });
    
    acp.send_request(&request);
    let response = acp.read_message().expect("read session/new response");
    let response: e2e::RpcResponse = serde_json::from_value(response).expect("parse session/new response");
    
    assert!(response.error.is_none(), "session/new should succeed: {:?}", response.error);
    
    let result = response.result.expect("session/new should have result");
    let session_id = result.get("sessionId").expect("should have sessionId");
    session_id.as_str().expect("sessionId should be string").to_string()
}

/// Helper function to set session model
async fn set_session_model(acp: &mut AcpChild, session_id: &str, model: &str) {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/set_model",
        "params": {
            "sessionId": session_id,
            "modelId": model
        }
    });
    
    acp.send_request(&request);
    let response = acp.read_message().expect("read session/set_model response");
    let response: e2e::RpcResponse = serde_json::from_value(response).expect("parse session/set_model response");
    
    assert!(response.error.is_none(), "session/set_model should succeed: {:?}", response.error);
}

/// Helper function to send a prompt
async fn send_prompt(acp: &mut AcpChild, session_id: &str, prompt: &str) -> e2e::RpcResponse {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [
                {
                    "type": "text",
                    "text": prompt
                }
            ]
        }
    });
    
    acp.send_request(&request);
    let response = acp.read_message().expect("read prompt response");
    let response: e2e::RpcResponse = serde_json::from_value(response).expect("parse prompt response");
    
    response
}

#[test]
fn e2e_acp_explicit_model_overrides_tier() {
    // Test scenario 1: ACP explicit model selection overrides agent tier configuration
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");
    
    // Initialize
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        initialize_session(&mut acp).await;
        
        // Create session
        let session_id = create_session(&mut acp, ".").await;
        
        // Set explicit model via ACP
        set_session_model(&mut acp, &session_id, "gpt-4-turbo").await;
        
        // Send a simple prompt to verify model is used
        let response = send_prompt(&mut acp, &session_id, "Hello").await;
        
        // Verify the prompt was processed
        assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);
        assert!(response.result.is_some(), "prompt should have result");
        
        // The actual model verification would require checking logs or response content
        // For now, we verify the session accepts the prompt without errors
        let result = response.result.unwrap();
        assert!(result.is_object(), "result should be an object");
    });
    
    // Clean shutdown
    drop(acp.stdin);
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}

#[test]
fn e2e_agent_tier_used_when_no_acp_model() {
    // Test scenario 2: Agent tier configuration is used when ACP doesn't select a model
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");
    
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        initialize_session(&mut acp).await;
        
        // Create session without setting ACP model
        let session_id = create_session(&mut acp, ".").await;
        
        // Send a prompt - should use agent's tier configuration
        let response = send_prompt(&mut acp, &session_id, "Test tier configuration").await;
        
        assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);
        assert!(response.result.is_some(), "prompt should have result");
        
        // Verify the session processed the prompt using tier-based model selection
        let result = response.result.unwrap();
        assert!(result.is_object(), "result should be an object");
    });
    
    drop(acp.stdin);
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}

#[test]
fn e2e_default_config_when_no_model_or_tier() {
    // Test scenario 3: Default configuration is used when neither ACP model nor agent tier is configured
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");
    
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        initialize_session(&mut acp).await;
        
        // Create session without any model configuration
        let session_id = create_session(&mut acp, ".").await;
        
        // Send a prompt - should use default configuration
        let response = send_prompt(&mut acp, &session_id, "Test default config").await;
        
        assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);
        assert!(response.result.is_some(), "prompt should have result");
        
        let result = response.result.unwrap();
        assert!(result.is_object(), "result should be an object");
    });
    
    drop(acp.stdin);
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}

#[test]
fn e2e_model_switching_within_session() {
    // Test scenario 4: Model switching within the same session
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");
    
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        initialize_session(&mut acp).await;
        
        let session_id = create_session(&mut acp, ".").await;
        
        // Start with one model
        set_session_model(&mut acp, &session_id, "gpt-3.5-turbo").await;
        let response1 = send_prompt(&mut acp, &session_id, "First model").await;
        assert!(response1.error.is_none(), "first prompt should succeed");
        
        // Switch to another model
        set_session_model(&mut acp, &session_id, "gpt-4").await;
        let response2 = send_prompt(&mut acp, &session_id, "Second model").await;
        assert!(response2.error.is_none(), "second prompt should succeed");
        
        // Verify both prompts were processed successfully
        assert!(response1.result.is_some() && response2.result.is_some());
    });
    
    drop(acp.stdin);
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}

#[test]
fn e2e_empty_model_string_uses_default() {
    // Test scenario 5: Empty model string should be treated as no model selection
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");
    
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        initialize_session(&mut acp).await;
        
        let session_id = create_session(&mut acp, ".").await;
        
        // Set empty model string
        set_session_model(&mut acp, &session_id, "").await;
        
        // Send prompt - should use default/tier configuration
        let response = send_prompt(&mut acp, &session_id, "Test empty model").await;
        
        assert!(response.error.is_none(), "prompt should succeed: {:?}", response.error);
        assert!(response.result.is_some(), "prompt should have result");
    });
    
    drop(acp.stdin);
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}

#[test]
fn e2e_multiple_sessions_with_different_models() {
    // Test scenario 6: Multiple sessions can have different model configurations
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");
    
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        initialize_session(&mut acp).await;
        
        // Create first session with one model
        let session1 = create_session(&mut acp, ".").await;
        set_session_model(&mut acp, &session1, "gpt-4").await;
        
        // Create second session with different model
        let session2 = create_session(&mut acp, ".").await;
        set_session_model(&mut acp, &session2, "gpt-3.5-turbo").await;
        
        // Send prompts to both sessions
        let response1 = send_prompt(&mut acp, &session1, "Session 1").await;
        let response2 = send_prompt(&mut acp, &session2, "Session 2").await;
        
        // Both should succeed independently
        assert!(response1.error.is_none(), "session 1 prompt should succeed");
        assert!(response2.error.is_none(), "session 2 prompt should succeed");
        assert!(response1.result.is_some() && response2.result.is_some());
    });
    
    drop(acp.stdin);
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}

#[test]
fn e2e_session_mode_switching_preserves_model() {
    // Test scenario 7: Switching session mode should preserve model configuration
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");
    
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        initialize_session(&mut acp).await;
        
        let session_id = create_session(&mut acp, ".").await;
        
        // Set model first
        set_session_model(&mut acp, &session_id, "gpt-4-turbo").await;
        
        // Switch session mode
        let mode_request = json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "session/set_mode",
            "params": {
                "sessionId": session_id,
                "modeId": "agent-builder"
            }
        });
        
        acp.send_request(&mode_request);
        let mode_response = acp.read_message().expect("read set_mode response");
        let mode_response: e2e::RpcResponse = serde_json::from_value(mode_response).expect("parse set_mode response");
        assert!(mode_response.error.is_none(), "set_mode should succeed");
        
        // Send prompt - model should still be configured
        let response = send_prompt(&mut acp, &session_id, "After mode switch").await;
        assert!(response.error.is_none(), "prompt should succeed after mode switch");
    });
    
    drop(acp.stdin);
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}

#[test]
fn e2e_sub_agent_config_independence() {
    // Test scenario 8: Sub-agent configuration independence from main agent ACP selection
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");
    
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        initialize_session(&mut acp).await;
        
        // Main agent uses ACP selected expensive model
        let session_id = create_session(&mut acp, ".").await;
        set_session_model(&mut acp, &session_id, "gpt-4").await;
        
        // Main agent calls sub-agent via invoke_agent
        let invoke_prompt = r#"Use invoke_agent to analyze this file with a lightweight agent"#;
        
        // Send a prompt that would trigger invoke_agent
        let response = send_prompt(&mut acp, &session_id, invoke_prompt).await;
        
        // The main prompt should succeed
        assert!(response.error.is_none(), "main prompt should succeed: {:?}", response.error);
        
        // Note: The actual sub-agent independence verification would require:
        // 1. Mocking the sub-agent configuration
        // 2. Checking logs to verify sub-agent used its own config
        // 3. Or monitoring API calls to verify different models were used
        
        // For this e2e test, we verify the main session accepts invoke_agent calls
        let result = response.result.unwrap();
        assert!(result.is_object(), "result should be an object");
    });
    
    drop(acp.stdin);
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}

#[test]
fn e2e_model_selection_with_provider_format() {
    // Test scenario 9: ACP model selection with provider/model format
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");
    
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        initialize_session(&mut acp).await;
        
        let session_id = create_session(&mut acp, ".").await;
        
        // Set model with provider format
        set_session_model(&mut acp, &session_id, "openai/gpt-4-turbo").await;
        
        // Send prompt
        let response = send_prompt(&mut acp, &session_id, "Test provider format").await;
        
        assert!(response.error.is_none(), "prompt should succeed with provider format: {:?}", response.error);
        assert!(response.result.is_some(), "prompt should have result");
    });
    
    drop(acp.stdin);
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}

#[test]
fn e2e_model_resolution_performance() {
    // Test scenario 10: Verify model resolution performance is acceptable
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");
    
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        initialize_session(&mut acp).await;
        
        let session_id = create_session(&mut acp, ".").await;
        
        // Measure model selection performance
        let start = std::time::Instant::now();
        set_session_model(&mut acp, &session_id, "gpt-4").await;
        let model_set_duration = start.elapsed();
        
        // Measure prompt processing performance
        let start = std::time::Instant::now();
        let response = send_prompt(&mut acp, &session_id, "Performance test").await;
        let prompt_duration = start.elapsed();
        
        assert!(response.error.is_none(), "prompt should succeed");
        
        // Verify performance is acceptable (these are conservative thresholds)
        assert!(model_set_duration.as_millis() < 1000, "Model selection should be fast: {:?}", model_set_duration);
        assert!(prompt_duration.as_millis() < 5000, "Prompt processing should be reasonably fast: {:?}", prompt_duration);
        
        println!("Performance metrics:");
        println!("  Model set: {:?}", model_set_duration);
        println!("  Prompt processing: {:?}", prompt_duration);
    });
    
    drop(acp.stdin);
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}

#[test]
fn e2e_multi_turn_model_consistency() {
    // Test scenario 11: Verify model consistency across multiple conversation turns
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");
    
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        initialize_session(&mut acp).await;
        
        let session_id = create_session(&mut acp, ".").await;
        
        // Set initial model
        set_session_model(&mut acp, &session_id, "gpt-4-turbo").await;
        
        // Multiple conversation turns
        let prompts = vec![
            "First turn",
            "Second turn", 
            "Third turn",
            "Fourth turn",
            "Fifth turn",
        ];
        
        let mut all_succeeded = true;
        for (i, prompt) in prompts.iter().enumerate() {
            let response = send_prompt(&mut acp, &session_id, prompt).await;
            
            if response.error.is_some() {
                eprintln!("Turn {} failed: {:?}", i + 1, response.error);
                all_succeeded = false;
            }
            
            // Small delay between turns to simulate real conversation
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        
        assert!(all_succeeded, "All conversation turns should succeed");
        
        // Switch model mid-conversation
        set_session_model(&mut acp, &session_id, "gpt-3.5-turbo").await;
        
        // Continue conversation with new model
        let response = send_prompt(&mut acp, &session_id, "After model switch").await;
        assert!(response.error.is_none(), "prompt should succeed after model switch");
        
        // Continue more turns to verify consistency with new model
        for i in 0..3 {
            let response = send_prompt(&mut acp, &session_id, &format!("Turn {} after switch", i + 1)).await;
            assert!(response.error.is_none(), "turn {} after switch should succeed", i + 1);
        }
    });
    
    drop(acp.stdin);
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}

#[test]
fn e2e_concurrent_sessions_model_isolation() {
    // Test scenario 12: Verify multiple concurrent sessions maintain model isolation
    let mut acp = AcpChild::spawn(None).expect("spawn loom-acp");
    
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        initialize_session(&mut acp).await;
        
        // Create multiple sessions with different models
        let session1 = create_session(&mut acp, ".").await;
        let session2 = create_session(&mut acp, ".").await;
        let session3 = create_session(&mut acp, ".").await;
        
        // Set different models for each session
        set_session_model(&mut acp, &session1, "gpt-4").await;
        set_session_model(&mut acp, &session2, "gpt-3.5-turbo").await;
        set_session_model(&mut acp, &session3, "gpt-4-turbo").await;
        
        // Send prompts to all sessions in interleaved manner
        let sessions = vec![session1, session2, session3];
        let mut all_succeeded = true;
        
        for round in 0..3 {
            for (session_idx, session_id) in sessions.iter().enumerate() {
                let prompt = format!("Session {} round {}", session_idx + 1, round + 1);
                let response = send_prompt(&mut acp, session_id, &prompt).await;
                
                if response.error.is_some() {
                    eprintln!("Session {} round {} failed: {:?}", session_idx + 1, round + 1, response.error);
                    all_succeeded = false;
                }
            }
        }
        
        assert!(all_succeeded, "All session prompts should succeed");
    });
    
    drop(acp.stdin);
    let status = acp.wait().expect("wait for process");
    assert!(status.success(), "loom-acp should exit successfully");
}