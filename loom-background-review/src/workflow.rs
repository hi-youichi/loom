//! Background review workflow infrastructure
//!
//! Provides the global `PendingReviewRegistry` and Curator auto-run helper.
//! The actual review execution lives in `crate::review`.

use super::curator::{Curator, CuratorConfig};
use super::skill_registry::{default_path as skills_default_path, SkillRegistry};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tracing::{info, warn};

/// Global registry to track pending background review tasks.
static PENDING_REVIEWS: std::sync::OnceLock<Arc<PendingReviewRegistry>> =
    std::sync::OnceLock::new();

fn pending_reviews() -> &'static Arc<PendingReviewRegistry> {
    PENDING_REVIEWS.get_or_init(|| Arc::new(PendingReviewRegistry::new()))
}

/// Registry of pending background review handles.
pub struct PendingReviewRegistry {
    handles: StdMutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl PendingReviewRegistry {
    fn new() -> Self {
        Self {
            handles: StdMutex::new(Vec::new()),
        }
    }

    pub fn push(&self, handle: tokio::task::JoinHandle<()>) {
        let mut handles = self.handles.lock().unwrap();
        handles.push(handle);
    }

    pub async fn wait_all(&self) -> usize {
        let handles: Vec<_> = {
            let mut handles = self.handles.lock().unwrap();
            handles.drain(..).collect()
        };
        let count = handles.len();
        if count == 0 {
            return 0;
        }

        info!("Waiting for {} background review(s) to complete...", count);

        const WAIT_TIMEOUT: Duration = Duration::from_secs(60);
        for handle in handles {
            match tokio::time::timeout(WAIT_TIMEOUT, handle).await {
                Ok(_) => {}
                Err(_) => warn!(
                    timeout = ?WAIT_TIMEOUT,
                    "Background review task did not finish in time, abandoning"
                ),
            }
        }

        info!("All {} background review(s) completed.", count);
        count
    }
}

/// Callback type for review output notifications.
pub type ReviewOutputFn = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Default)]
pub struct BackgroundReviewCallbacks {
    pub on_output: Option<ReviewOutputFn>,
    pub on_review_complete: Option<ReviewOutputFn>,
}

pub async fn wait_for_pending_reviews() -> usize {
    pending_reviews().wait_all().await
}

pub fn run_curator_if_needed(
    skills_path: &std::path::Path,
    curator_config: &CuratorConfig,
) -> Result<(), String> {
    let skills = SkillRegistry::new(skills_path);
    let state_path = skills_path.join("curator").join("state.json");
    let curator = Curator::new(skills, curator_config.clone()).with_state_path(state_path);

    if !curator.should_run(None) {
        return Ok(());
    }

    let report = curator.run(false).map_err(|e| format!("{:?}", e))?;

    if !report.stale.is_empty() || !report.archived.is_empty() || !report.overlapping.is_empty() {
        info!(
            "Curator: {} active, {} stale, {} archived, {} overlapping",
            report.active,
            report.stale.len(),
            report.archived.len(),
            report.overlapping.len(),
        );
    }

    Ok(())
}

pub fn skills_default_path_public() -> std::path::PathBuf {
    skills_default_path()
}
