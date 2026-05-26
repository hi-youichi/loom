use loom::background_review::{
    AgentReviewConfig, AgentReviewRunner, MemoryFile, MemoryStore, ReviewMode, SkillRegistry,
};
use loom::{MultiRoundMockLlm, ToolCall};

fn tool_call(name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        name: name.to_string(),
        arguments: arguments.to_string(),
        id: Some("call-1".to_string()),
    }
}

fn review_config() -> AgentReviewConfig {
    AgentReviewConfig {
        max_iterations: 16,
        max_session_chars: 24000,
        mode: ReviewMode::Agent,
        review_memory: true,
        review_skills: true,
    }
}

fn sample_session() -> &'static str {
    "User: I prefer dark mode in all my editors.\nAssistant: Got it, I'll remember that preference."
}

#[tokio::test(flavor = "current_thread")]
async fn review_saves_user_preference_to_memory() {
    let dir = tempfile::tempdir().unwrap();
    let memory = MemoryStore::new(dir.path());
    let skills = SkillRegistry::new(&dir.path().join("skills"));

    let llm = MultiRoundMockLlm::new(vec![
        (
            "Checking memory...".into(),
            vec![tool_call(
                "memory_set",
                r#"{"file":"user","action":"append","content":"- prefers dark mode"}"#,
            )],
        ),
        ("Nothing more to save.".into(), vec![]),
    ]);

    let result = AgentReviewRunner::run_with_refs(
        &llm as &dyn loom::LlmClient,
        &memory,
        &skills,
        sample_session(),
        &review_config(),
    )
    .await
    .unwrap();

    assert_eq!(result.actions.len(), 1);
    assert_eq!(result.actions[0].kind, "memory");
    let content = memory.load(MemoryFile::User).unwrap();
    assert!(content.contains("dark mode"));
}

#[tokio::test(flavor = "current_thread")]
async fn review_no_updates_when_nothing_to_save() {
    let dir = tempfile::tempdir().unwrap();
    let memory = MemoryStore::new(dir.path());
    let skills = SkillRegistry::new(&dir.path().join("skills"));

    let llm = MultiRoundMockLlm::new(vec![("Nothing to save.".into(), vec![])]);

    let result = AgentReviewRunner::run_with_refs(
        &llm as &dyn loom::LlmClient,
        &memory,
        &skills,
        sample_session(),
        &review_config(),
    )
    .await
    .unwrap();

    assert!(result.actions.is_empty());
    assert_eq!(result.summary, "No updates.");
}

#[tokio::test(flavor = "current_thread")]
async fn review_respects_max_iterations() {
    let dir = tempfile::tempdir().unwrap();
    let memory = MemoryStore::new(dir.path());
    let skills = SkillRegistry::new(&dir.path().join("skills"));

    let llm = MultiRoundMockLlm::new(vec![
        (
            "Looping...".into(),
            vec![tool_call(
                "memory_set",
                r#"{"file":"facts","action":"append","content":"iteration"}"#,
            )],
        ),
    ]);

    let config = AgentReviewConfig {
        max_iterations: 3,
        ..review_config()
    };

    let result = AgentReviewRunner::run_with_refs(
        &llm as &dyn loom::LlmClient,
        &memory,
        &skills,
        sample_session(),
        &config,
    )
    .await
    .unwrap();

    assert_eq!(result.iterations, 3);
    assert_eq!(result.actions.len(), 3);
}

fn skill_create_args() -> String {
    serde_json::json!({
        "name": "rust-debugging",
        "description": "Debug Rust issues",
        "triggers": ["rust", "debug", "panic"],
        "body": "# Rust Debugging\n\nSteps to debug Rust issues."
    }).to_string()
}

#[tokio::test(flavor = "current_thread")]
async fn review_creates_new_skill() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join("skills");
    let memory = MemoryStore::new(dir.path());
    let skills = SkillRegistry::new(&skills_dir);

    let llm = MultiRoundMockLlm::new(vec![
        (
            "Creating skill...".into(),
            vec![tool_call("skill_create", &skill_create_args())],
        ),
        ("Done.".into(), vec![]),
    ]);

    let result = AgentReviewRunner::run_with_refs(
        &llm as &dyn loom::LlmClient,
        &memory,
        &skills,
        "User: How to debug a Rust panic?\nAssistant: Let me help with Rust debugging.",
        &review_config(),
    )
    .await
    .unwrap();

    assert_eq!(result.actions.len(), 1);
    assert_eq!(result.actions[0].kind, "skill");
    assert!(result.actions[0].summary.contains("created"));
    let loaded = skills.load("rust-debugging").unwrap();
    assert_eq!(loaded.name, "rust-debugging");
}

#[tokio::test(flavor = "current_thread")]
async fn review_truncates_long_session() {
    let dir = tempfile::tempdir().unwrap();
    let memory = MemoryStore::new(dir.path());
    let skills = SkillRegistry::new(&dir.path().join("skills"));

    let llm = MultiRoundMockLlm::new(vec![("Nothing to save.".into(), vec![])]);

    let long_session: String = "X".repeat(50000);
    let config = AgentReviewConfig {
        max_session_chars: 1000,
        ..review_config()
    };

    let result = AgentReviewRunner::run_with_refs(
        &llm as &dyn loom::LlmClient,
        &memory,
        &skills,
        &long_session,
        &config,
    )
    .await
    .unwrap();

    assert!(result.iterations >= 1);
}

#[tokio::test(flavor = "current_thread")]
async fn review_rejects_disallowed_tools() {
    let dir = tempfile::tempdir().unwrap();
    let memory = MemoryStore::new(dir.path());
    let skills = SkillRegistry::new(&dir.path().join("skills"));

    let llm = MultiRoundMockLlm::new(vec![
        (
            "Trying bash...".into(),
            vec![tool_call("bash", r#"{"command":"rm -rf /"}"#)],
        ),
        ("Nothing to save.".into(), vec![]),
    ]);

    let result = AgentReviewRunner::run_with_refs(
        &llm as &dyn loom::LlmClient,
        &memory,
        &skills,
        sample_session(),
        &review_config(),
    )
    .await
    .unwrap();

    let tool_results: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m["role"].as_str() == Some("tool"))
        .collect();
    if let Some(first_result) = tool_results.first() {
        let success = first_result["result"]["success"].as_bool().unwrap_or(true);
        assert!(!success, "Disallowed tool should fail");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn review_multi_tool_call_in_single_turn() {
    let dir = tempfile::tempdir().unwrap();
    let memory = MemoryStore::new(dir.path());
    let skills = SkillRegistry::new(&dir.path().join("skills"));

    let llm = MultiRoundMockLlm::new(vec![
        (
            "Updating memory and facts...".into(),
            vec![
                tool_call(
                    "memory_set",
                    r#"{"file":"user","action":"append","content":"- prefers Rust"}"#,
                ),
                tool_call(
                    "memory_set",
                    r#"{"file":"facts","action":"append","content":"- uses Windows"}"#,
                ),
            ],
        ),
        ("Done.".into(), vec![]),
    ]);

    let result = AgentReviewRunner::run_with_refs(
        &llm as &dyn loom::LlmClient,
        &memory,
        &skills,
        sample_session(),
        &review_config(),
    )
    .await
    .unwrap();

    assert_eq!(result.actions.len(), 2);
    assert!(memory.load(MemoryFile::User).unwrap().contains("Rust"));
    assert!(memory.load(MemoryFile::Facts).unwrap().contains("Windows"));
}
