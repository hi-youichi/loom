//! Multi-workspace support for LSP Manager.
//!
//! Allows managing multiple project workspaces simultaneously, each with its own
//! set of language servers and configurations.

use dashmap::DashMap;
use std::path::{Path, PathBuf};
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

/// Represents a workspace with its own configuration.
pub struct Workspace {
    /// Unique workspace identifier
    pub id: String,
    
    /// Root path of the workspace
    pub root_path: PathBuf,
    
    /// Workspace name (optional)
    pub name: Option<String>,
}

impl Workspace {
    /// Create a new workspace.
    pub fn new(id: String, root_path: PathBuf, name: Option<String>) -> Result<Self, WorkspaceError> {
        if !root_path.exists() {
            return Err(WorkspaceError::InvalidPath(
                format!("Path does not exist: {}", root_path.display())
            ));
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
            *active = self.workspaces.iter().next().map(|entry| entry.key().clone());
        }

        Ok(())
    }

    /// Get a workspace by ID.
    pub fn get_workspace(&self, id: &str) -> Option<Arc<Workspace>> {
        self.workspaces.get(id).map(|entry| Arc::clone(entry.value()))
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
        self.workspaces.iter().map(|entry| Arc::clone(entry.value())).collect()
    }
}

impl Default for WorkspaceManager {
    fn default() -> Self {
        Self::new()
    }
}
