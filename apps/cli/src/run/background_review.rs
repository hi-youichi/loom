//! CLI wrapper for background review.

pub use loom::background_review::workflow::{
    BackgroundReviewConfig, BackgroundReviewHandle,
    build_background_config_from_opts,
    ReviewOutputFn,
};

/// Spawn a background review with CLI output (eprintln).
///
/// Hermes 对齐：background review 不向用户终端输出任何内容.
/// 当前保留 eprintln 用于调试，后续可移除。
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
