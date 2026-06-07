use std::path::PathBuf;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher, Event, EventKind, Config as NotifyConfig};
use tokio::sync::broadcast;
use tracing::{info, warn};

use loom_protocol::responses::{WorkspaceFileChangedResponse, FileChange};

const DEBOUNCE_MS: u64 = 100;

pub struct WorkspaceWatcher {
    _watcher: RecommendedWatcher,
    #[allow(dead_code)]
    workspace_id: String,
}

impl WorkspaceWatcher {
    pub fn start(
        workspace_id: String,
        root_dir: PathBuf,
        tx: broadcast::Sender<WorkspaceFileChangedResponse>,
    ) -> Result<Self, notify::Error> {
        let wid = workspace_id.clone();
        let root = root_dir.clone();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        if event.paths.is_empty() {
                            return;
                        }

                        let kind = match event.kind {
                            EventKind::Create(_) => "create",
                            EventKind::Modify(_) => "modify",
                            EventKind::Remove(_) => "delete",
                            EventKind::Any => "any",
                            _ => return,
                        };

                        let changes: Vec<FileChange> = event
                            .paths
                            .iter()
                            .filter(|p| {
                                let rel = p.strip_prefix(&root).unwrap_or(p);
                                let rel_str = rel.to_string_lossy();
                                !rel_str.starts_with(".git")
                                    && !rel_str.starts_with("node_modules")
                                    && !rel_str.ends_with(".tmp")
                            })
                            .filter_map(|p| {
                                let rel = p.strip_prefix(&root).unwrap_or(p);
                                let rel_str = rel.to_string_lossy().to_string();
                                if rel_str.is_empty() {
                                    None
                                } else {
                                    Some(FileChange {
                                        path: rel_str,
                                        kind: kind.to_string(),
                                    })
                                }
                            })
                            .collect();

                        if changes.is_empty() {
                            return;
                        }

                        let notification = WorkspaceFileChangedResponse::new(
                            wid.clone(),
                            changes,
                        );

                        tracing::info!("📤 File change detected for workspace {}: {} changes", wid, notification.changes.len());

                        let _ = tx.send(notification);
                    }
                    Err(e) => {
                        warn!("File watch error: {}", e);
                    }
                }
            },
            NotifyConfig::default().with_poll_interval(Duration::from_millis(DEBOUNCE_MS)),
        )?;

        watcher.watch(&root_dir, RecursiveMode::Recursive)?;

        info!("Started file watcher for workspace {} at {:?}", workspace_id, root_dir);

        Ok(Self {
            _watcher: watcher,
            workspace_id,
        })
    }

    #[allow(dead_code)]
    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }
}

pub fn workspace_root_dir() -> Option<PathBuf> {
    std::env::var("WORKSPACE_ROOT_DIR")
        .ok()
        .map(PathBuf::from)
}

pub fn workspace_dir(workspace_id: &str) -> Option<PathBuf> {
    workspace_root_dir().map(|root| root.join(workspace_id))
}
