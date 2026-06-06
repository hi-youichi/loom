//! CLI wrapper for background review.

pub use loom::background_review::workflow::{
    BackgroundReviewConfig, BackgroundReviewHandle,
    build_background_config_from_opts_ext,
    wait_for_pending_reviews, PendingReviewRegistry,
    ReviewOutputFn,
};

/// Spawn a background review with CLI output (eprintln).
pub fn spawn_background_review(
    config: BackgroundReviewConfig,
    session_content: String,
    session_id: String,
) {
    let on_output: ReviewOutputFn = std::sync::Arc::new(|msg: &str| {
        eprintln!("\n📚 {}", msg);
    });
    loom::background_review::spawn_background_review(
        config, session_content, session_id, Some(on_output),
    );
}
