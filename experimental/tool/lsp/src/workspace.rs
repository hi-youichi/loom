//! Multi-workspace support for LSP Manager.
//!
//! Allows managing multiple project workspaces simultaneously, each with its own
//! set of language servers and configurations.

use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("Workspace not found: {0}")]
    NotFound(String),

    #[error("Workspace already exists: {0}")]
    AlreadyExists(String),

    #[error("Invalid workspace path: {0}")]
    InvalidPath(String),
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: String,

    /// Root path of the workspace
    pub root_path: PathBuf,

    /// Workspace name (optional)
    pub name: Option<String>,
}

impl Workspace {
    /// Create a new workspace.
    pub fn new(
        id: String,
        root_path: PathBuf,
        name: Option<String>,
    ) -> Result<Self, WorkspaceError> {
        if !root_path.exists() {
            return Err(WorkspaceError::InvalidPath(format!(
                "Path does not exist: {}",
                root_path.display()
            )));
        }

        Ok(Self {
            id,
            root_path,
            name,
        })
    }
}

/// Manages multiple workspaces.
pub struct WorkspaceManager {
    workspaces: DashMap<String, Arc<Workspace>>,
    active_workspace: RwLock<Option<String>>,
}

impl WorkspaceManager {
    /// Create a new workspace manager.
    pub fn new() -> Self {
        Self {
            workspaces: DashMap::new(),
            active_workspace: RwLock::new(None),
        }
    }

    /// Add a new workspace.
    pub async fn add_workspace(
        &self,
        id: String,
        root_path: PathBuf,
        name: Option<String>,
    ) -> Result<Arc<Workspace>, WorkspaceError> {
        if self.workspaces.contains_key(&id) {
            return Err(WorkspaceError::AlreadyExists(id));
        }

        let workspace = Arc::new(Workspace::new(id.clone(), root_path, name)?);
        self.workspaces.insert(id.clone(), Arc::clone(&workspace));

        // Set as active if it's the first workspace
        if self.workspaces.len() == 1 {
            let mut active = self.active_workspace.write().await;
            *active = Some(id);
        }

        Ok(workspace)
    }

    /// Remove a workspace.
    pub async fn remove_workspace(&self, id: &str) -> Result<(), WorkspaceError> {
        if self.workspaces.remove(id).is_none() {
            return Err(WorkspaceError::NotFound(id.to_string()));
        }

        // Update active workspace if needed
        let mut active = self.active_workspace.write().await;
        if active.as_ref() == Some(&id.to_string()) {
            *active = self
                .workspaces
                .iter()
                .next()
                .map(|entry| entry.key().clone());
        }

        Ok(())
    }

    /// Get a workspace by ID.
    pub fn get_workspace(&self, id: &str) -> Option<Arc<Workspace>> {
        self.workspaces
            .get(id)
            .map(|entry| Arc::clone(entry.value()))
    }

    /// Get the active workspace.
    pub async fn get_active_workspace(&self) -> Option<Arc<Workspace>> {
        let active = self.active_workspace.read().await;
        active.as_ref().and_then(|id| self.get_workspace(id))
    }

    /// Set the active workspace.
    pub async fn set_active_workspace(&self, id: &str) -> Result<(), WorkspaceError> {
        if !self.workspaces.contains_key(id) {
            return Err(WorkspaceError::NotFound(id.to_string()));
        }

        let mut active = self.active_workspace.write().await;
        *active = Some(id.to_string());

        Ok(())
    }

    /// List all workspaces.
    pub fn list_workspaces(&self) -> Vec<Arc<Workspace>> {
        self.workspaces
            .iter()
            .map(|entry| Arc::clone(entry.value()))
            .collect()
    }
}

impl Default for WorkspaceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_new_with_valid_path() {
        let temp_dir = std::env::temp_dir();
        let id = "test_workspace".to_string();
        let root_path = temp_dir.clone();
        let name = Some("Test Workspace".to_string());

        let workspace = Workspace::new(id.clone(), root_path, name.clone());

        assert!(workspace.is_ok());
        let workspace = workspace.unwrap();
        assert_eq!(workspace.id, id);
        assert_eq!(workspace.name, name);
        assert_eq!(workspace.root_path, temp_dir);
    }

    #[test]
    fn test_workspace_new_with_invalid_path() {
        let id = "test_workspace".to_string();
        let root_path = PathBuf::from("/nonexistent/path/that/does/not/exist");
        let name = Some("Test Workspace".to_string());

        let workspace = Workspace::new(id.clone(), root_path, name);

        assert!(workspace.is_err());
        match workspace.unwrap_err() {
            WorkspaceError::InvalidPath(msg) => {
                assert!(msg.contains("Path does not exist"));
            }
            _ => panic!("Expected InvalidPath error"),
        }
    }

    #[test]
    fn test_workspace_manager_new() {
        let manager = WorkspaceManager::new();
        assert_eq!(manager.workspaces.len(), 0);
    }

    #[test]
    fn test_workspace_manager_default() {
        let manager = WorkspaceManager::default();
        assert_eq!(manager.workspaces.len(), 0);
    }

    #[tokio::test]
    async fn test_workspace_manager_add_workspace() {
        let temp_dir = std::env::temp_dir();
        let manager = WorkspaceManager::new();

        let result = manager
            .add_workspace(
                "test1".to_string(),
                temp_dir.clone(),
                Some("Test Workspace 1".to_string()),
            )
            .await;

        assert!(result.is_ok());
        let workspace = result.unwrap();
        assert_eq!(workspace.id, "test1");
        assert_eq!(manager.workspaces.len(), 1);
    }

    #[tokio::test]
    async fn test_workspace_manager_add_duplicate_workspace() {
        let temp_dir = std::env::temp_dir();
        let manager = WorkspaceManager::new();

        let _ = manager
            .add_workspace("test1".to_string(), temp_dir.clone(), None)
            .await;

        let result = manager
            .add_workspace("test1".to_string(), temp_dir.clone(), None)
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            WorkspaceError::AlreadyExists(id) => {
                assert_eq!(id, "test1");
            }
            _ => panic!("Expected AlreadyExists error"),
        }
    }

    #[tokio::test]
    async fn test_workspace_manager_remove_workspace() {
        let temp_dir = std::env::temp_dir();
        let manager = WorkspaceManager::new();

        let _ = manager
            .add_workspace("test1".to_string(), temp_dir.clone(), None)
            .await;

        let result = manager.remove_workspace("test1").await;

        assert!(result.is_ok());
        assert_eq!(manager.workspaces.len(), 0);
    }

    #[tokio::test]
    async fn test_workspace_manager_remove_nonexistent_workspace() {
        let manager = WorkspaceManager::new();

        let result = manager.remove_workspace("nonexistent").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            WorkspaceError::NotFound(id) => {
                assert_eq!(id, "nonexistent");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_workspace_manager_get_workspace() {
        let temp_dir = std::env::temp_dir();
        let manager = WorkspaceManager::new();

        let _ = manager
            .add_workspace("test1".to_string(), temp_dir.clone(), None)
            .await;

        let workspace = manager.get_workspace("test1");

        assert!(workspace.is_some());
        assert_eq!(workspace.unwrap().id, "test1");
    }

    #[tokio::test]
    async fn test_workspace_manager_get_nonexistent_workspace() {
        let manager = WorkspaceManager::new();

        let workspace = manager.get_workspace("nonexistent");

        assert!(workspace.is_none());
    }

    #[tokio::test]
    async fn test_workspace_manager_get_active_workspace_first() {
        let temp_dir = std::env::temp_dir();
        let manager = WorkspaceManager::new();

        let _ = manager
            .add_workspace("test1".to_string(), temp_dir.clone(), None)
            .await;

        let active = manager.get_active_workspace().await;

        assert!(active.is_some());
        assert_eq!(active.unwrap().id, "test1");
    }

    #[tokio::test]
    async fn test_workspace_manager_get_active_workspace_multiple() {
        let temp_dir = std::env::temp_dir();
        let manager = WorkspaceManager::new();

        let _ = manager
            .add_workspace("test1".to_string(), temp_dir.clone(), None)
            .await;
        let _ = manager
            .add_workspace("test2".to_string(), temp_dir.clone(), None)
            .await;

        let active = manager.get_active_workspace().await;

        assert!(active.is_some());
        assert_eq!(active.unwrap().id, "test1"); // First should be active
    }

    #[tokio::test]
    async fn test_workspace_manager_set_active_workspace() {
        let temp_dir = std::env::temp_dir();
        let manager = WorkspaceManager::new();

        let _ = manager
            .add_workspace("test1".to_string(), temp_dir.clone(), None)
            .await;
        let _ = manager
            .add_workspace("test2".to_string(), temp_dir.clone(), None)
            .await;

        let result = manager.set_active_workspace("test2").await;

        assert!(result.is_ok());
        let active = manager.get_active_workspace().await;
        assert_eq!(active.unwrap().id, "test2");
    }

    #[tokio::test]
    async fn test_workspace_manager_set_active_workspace_nonexistent() {
        let manager = WorkspaceManager::new();

        let result = manager.set_active_workspace("nonexistent").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            WorkspaceError::NotFound(id) => {
                assert_eq!(id, "nonexistent");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_workspace_manager_list_workspaces() {
        let temp_dir = std::env::temp_dir();
        let manager = WorkspaceManager::new();

        let _ = manager
            .add_workspace("test1".to_string(), temp_dir.clone(), None)
            .await;
        let _ = manager
            .add_workspace("test2".to_string(), temp_dir.clone(), None)
            .await;
        let _ = manager
            .add_workspace("test3".to_string(), temp_dir.clone(), None)
            .await;

        let workspaces = manager.list_workspaces();

        assert_eq!(workspaces.len(), 3);
        let ids: Vec<&str> = workspaces.iter().map(|w| w.id.as_str()).collect();
        assert!(ids.contains(&"test1"));
        assert!(ids.contains(&"test2"));
        assert!(ids.contains(&"test3"));
    }

    #[tokio::test]
    async fn test_workspace_manager_remove_active_workspace_sets_next() {
        let temp_dir = std::env::temp_dir();
        let manager = WorkspaceManager::new();

        let _ = manager
            .add_workspace("test1".to_string(), temp_dir.clone(), None)
            .await;
        let _ = manager
            .add_workspace("test2".to_string(), temp_dir.clone(), None)
            .await;
        let _ = manager
            .add_workspace("test3".to_string(), temp_dir.clone(), None)
            .await;

        manager.set_active_workspace("test2").await.unwrap();
        manager.remove_workspace("test2").await.unwrap();

        let active = manager.get_active_workspace().await;

        assert!(active.is_some());
        let active_id = &active.unwrap().id;
        assert!(active_id == "test1" || active_id == "test3"); // Should be one of the remaining
    }

    #[tokio::test]
    async fn test_workspace_manager_empty_list() {
        let manager = WorkspaceManager::new();

        let workspaces = manager.list_workspaces();

        assert_eq!(workspaces.len(), 0);
    }

    #[tokio::test]
    async fn test_workspace_manager_no_active_workspace() {
        let manager = WorkspaceManager::new();

        let active = manager.get_active_workspace().await;

        assert!(active.is_none());
    }

    #[test]
    fn test_workspace_error_formatting() {
        let error = WorkspaceError::NotFound("test_workspace".to_string());
        assert!(error.to_string().contains("test_workspace"));
        assert!(error.to_string().contains("not found"));

        let error = WorkspaceError::AlreadyExists("test_workspace".to_string());
        assert!(error.to_string().contains("test_workspace"));
        assert!(error.to_string().contains("already exists"));

        let error = WorkspaceError::InvalidPath("/invalid/path".to_string());
        assert!(error.to_string().contains("/invalid/path"));
        assert!(error.to_string().contains("Invalid workspace path"));
    }

    #[test]
    fn test_workspace_struct_fields() {
        let temp_dir = std::env::temp_dir();
        let workspace = Workspace {
            id: "test".to_string(),
            root_path: temp_dir.clone(),
            name: Some("Test".to_string()),
        };

        assert_eq!(workspace.id, "test");
        assert_eq!(workspace.root_path, temp_dir);
        assert_eq!(workspace.name, Some("Test".to_string()));
    }

    #[tokio::test]
    async fn test_workspace_manager_concurrent_operations() {
        let temp_dir = std::env::temp_dir();
        let manager = Arc::new(WorkspaceManager::new());

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let manager = Arc::clone(&manager);
                let temp_dir = temp_dir.clone();
                tokio::spawn(async move {
                    manager
                        .add_workspace(format!("workspace_{}", i), temp_dir.clone(), None)
                        .await
                })
            })
            .collect();

        let results: Vec<_> = futures::future::join_all(handles)
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        assert!(results.iter().all(|r| r.is_ok()));
        assert_eq!(manager.workspaces.len(), 10);
    }
}
