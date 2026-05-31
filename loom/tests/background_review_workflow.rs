// Tests for background_review_workflow require the `testing` feature
// to access run_background_review_workflow.
#![cfg(feature = "testing")]

use loom::background_review::{
    run_background_review_workflow, BackgroundReviewConfig,
};

fn base_config() -> BackgroundReviewConfig {
    BackgroundReviewConfig {
        enabled: true,
        max_session_chars: 24000,
        max_iterations: 16,
        min_session_chars: 200,
        base_url: "https://mock.test".to_string(),
        api_key: "test-key".to_string(),
        model: "mock-model".to_string(),
        session_model: None,
        review_memory: true,
        review_skills: true,
        curator_config: Default::default(),
        curator_run_interval_secs: 86400,
        observability_enabled: false,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn workflow_skips_when_disabled() {
    let config = BackgroundReviewConfig {
        enabled: false,
        ..base_config()
    };
    let result: Result<_, String> = run_background_review_workflow(&config, "some content", "test-session").await;
    let (summary, actions, _, _, _) = result.unwrap();
    assert_eq!(summary, "disabled");
    assert_eq!(actions, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn workflow_skips_short_session() {
    let config = BackgroundReviewConfig {
        min_session_chars: 200,
        ..base_config()
    };
    let short = "hi".repeat(10);
    let result: Result<_, String> = run_background_review_workflow(&config, &short, "test-session").await;
    let (summary, actions, _, _, _) = result.unwrap();
    assert_eq!(summary, "session too short");
    assert_eq!(actions, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn workflow_skips_no_credentials() {
    let config = BackgroundReviewConfig {
        base_url: String::new(),
        api_key: String::new(),
        ..base_config()
    };
    let long = "x".repeat(300);
    let result: Result<_, String> = run_background_review_workflow(&config, &long, "test-session").await;
    let (summary, actions, _, _, _) = result.unwrap();
    assert_eq!(summary, "no credentials");
    assert_eq!(actions, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn workflow_skips_when_no_review_mode_enabled() {
    let config = BackgroundReviewConfig {
        review_memory: false,
        review_skills: false,
        ..base_config()
    };
    let long = "x".repeat(300);
    let result: Result<_, String> = run_background_review_workflow(&config, &long, "test-session").await;
    let (summary, actions, _, _, _) = result.unwrap();
    assert_eq!(summary, "no review mode enabled");
    assert_eq!(actions, 0);
}
