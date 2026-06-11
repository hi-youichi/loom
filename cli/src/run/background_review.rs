//! CLI wrapper for background review.

pub use loom_background_review::workflow::{
    BackgroundReviewConfig, BackgroundReviewHandle,
    build_background_config_from_opts_ext,
    wait_for_pending_reviews, PendingReviewRegistry,
    ReviewOutputFn, BackgroundReviewCallbacks,
};

pub fn spawn_background_review(
    config: BackgroundReviewConfig,
    session_content: String,
    session_id: String,
) {
    let on_output: ReviewOutputFn = std::sync::Arc::new(|msg: &str| {
        eprintln!("\n📚 {}", msg);
    });
    loom_background_review::spawn_background_review(
        config, session_content, session_id, BackgroundReviewCallbacks {
            on_output: Some(on_output),
            on_review_complete: None,
        },
    );
}
