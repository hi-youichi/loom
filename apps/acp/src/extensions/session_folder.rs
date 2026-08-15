use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::pagination::{encode_cursor, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const MAX_SORT_ORDER: i32 = 1_000_000;
const ORDER_VERSION: &str = "session-folder-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRef {
    pub session_id: String,
    pub title: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFolder {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
    pub session_count: u32,
    #[serde(default)]
    pub sessions: Vec<SessionRef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionFolderListRequest {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFolderListResponse {
    pub items: Vec<SessionFolder>,
    pub unassigned_count: u32,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionFolderCreateRequest {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFolderCreateResponse {
    pub folder: SessionFolder,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionFolderUpdateRequest {
    pub folder_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub color: Option<Option<String>>,
    #[serde(default)]
    pub sort_order: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFolderUpdateResponse {
    pub updated: bool,
    pub folder: SessionFolder,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionFolderDeleteRequest {
    pub folder_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFolderDeleteResponse {
    pub deleted: bool,
    pub folder_id: String,
    pub released_sessions: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionFolderAssignRequest {
    pub session_id: String,
    pub folder_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFolderAssignResponse {
    pub assigned: bool,
    pub session_id: String,
    pub folder_id: Option<String>,
    pub previous_folder_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionFolderChange {
    Create,
    Update,
    Delete,
    Assign,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFolderChangedParams {
    pub change: SessionFolderChange,
    pub folder_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FolderCursor {
    pub offset: usize,
    pub order: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StableFolderCursor {
    sort_order: i32,
    folder_id: String,
    order: String,
}

#[derive(Debug, Clone)]
pub struct FolderSnapshot {
    pub folders: Vec<SessionFolder>,
    pub unassigned_count: u32,
}

pub trait FolderStore: Send + Sync {
    fn snapshot(&self) -> Result<FolderSnapshot, String>;
    fn create(
        &self,
        name: String,
        color: Option<String>,
        sort_order: Option<i32>,
    ) -> Result<SessionFolder, String>;
    fn update(
        &self,
        folder_id: &str,
        name: Option<String>,
        color: Option<Option<String>>,
        sort_order: Option<i32>,
    ) -> Result<SessionFolder, String>;
    fn delete(&self, folder_id: &str) -> Result<(bool, u32), String>;
    fn assign(&self, session_id: &str, folder_id: Option<&str>) -> Result<Option<String>, String>;
}

pub trait SessionFolderNotifier: Send + Sync {
    fn publish(&self, params: SessionFolderChangedParams) -> Result<(), String>;
}

#[derive(Default)]
pub struct MemoryFolderStore {
    state: Mutex<MemoryState>,
}

#[derive(Default)]
struct MemoryState {
    folders: BTreeMap<String, SessionFolder>,
    sessions: BTreeMap<String, SessionRef>,
    assignments: BTreeMap<String, String>,
}

impl MemoryFolderStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_session(&self, session: SessionRef) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "folder store lock poisoned")?;
        state.sessions.insert(session.session_id.clone(), session);
        Ok(())
    }
}

impl FolderStore for MemoryFolderStore {
    fn snapshot(&self) -> Result<FolderSnapshot, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "folder store lock poisoned")?;
        let mut folders: Vec<_> = state.folders.values().cloned().collect();
        folders.sort_by_key(|folder| (folder.sort_order, folder.id.clone()));
        for folder in &mut folders {
            folder.sessions = state
                .assignments
                .iter()
                .filter(|(_, id)| id.as_str() == folder.id.as_str())
                .filter_map(|(session_id, _)| state.sessions.get(session_id).cloned())
                .collect();
            folder.sessions.sort_by(|a, b| {
                b.updated_at
                    .cmp(&a.updated_at)
                    .then_with(|| a.session_id.cmp(&b.session_id))
            });
            folder.session_count = folder.sessions.len() as u32;
        }
        let assigned = state
            .assignments
            .keys()
            .filter(|session_id| state.sessions.contains_key(*session_id))
            .count();
        Ok(FolderSnapshot {
            folders,
            unassigned_count: state.sessions.len().saturating_sub(assigned) as u32,
        })
    }

    fn create(
        &self,
        name: String,
        color: Option<String>,
        sort_order: Option<i32>,
    ) -> Result<SessionFolder, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "folder store lock poisoned")?;
        let mut folders: Vec<_> = state.folders.values_mut().collect();
        folders.sort_by_key(|folder| (folder.sort_order, folder.id.clone()));
        let position = sort_order
            .unwrap_or(folders.len() as i32)
            .min(folders.len() as i32)
            .max(0) as usize;
        for (index, folder) in folders.iter_mut().enumerate() {
            if index >= position {
                folder.sort_order = folder.sort_order.saturating_add(1);
            }
        }
        let now = Utc::now().to_rfc3339();
        let folder = SessionFolder {
            id: Uuid::new_v4().to_string(),
            name,
            color,
            sort_order: position as i32,
            created_at: now.clone(),
            updated_at: now,
            session_count: 0,
            sessions: Vec::new(),
        };
        state.folders.insert(folder.id.clone(), folder.clone());
        Ok(folder)
    }

    fn update(
        &self,
        folder_id: &str,
        name: Option<String>,
        color: Option<Option<String>>,
        sort_order: Option<i32>,
    ) -> Result<SessionFolder, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "folder store lock poisoned")?;
        let old_order = state
            .folders
            .get(folder_id)
            .ok_or_else(|| "folder not found".to_string())?
            .sort_order;
        if let Some(target) = sort_order {
            let target = target
                .min(state.folders.len().saturating_sub(1) as i32)
                .max(0);
            if target < old_order {
                for folder in state.folders.values_mut() {
                    if folder.id != folder_id
                        && folder.sort_order >= target
                        && folder.sort_order < old_order
                    {
                        folder.sort_order += 1;
                    }
                }
            } else if target > old_order {
                for folder in state.folders.values_mut() {
                    if folder.id != folder_id
                        && folder.sort_order > old_order
                        && folder.sort_order <= target
                    {
                        folder.sort_order -= 1;
                    }
                }
            }
        }
        let max_order = state.folders.len().saturating_sub(1) as i32;
        let folder = state
            .folders
            .get_mut(folder_id)
            .ok_or_else(|| "folder not found".to_string())?;
        if let Some(name) = name {
            folder.name = name;
        }
        if let Some(color) = color {
            folder.color = color;
        }
        if let Some(sort_order) = sort_order {
            folder.sort_order = sort_order.min(max_order).max(0);
        }
        folder.updated_at = Utc::now().to_rfc3339();
        Ok(folder.clone())
    }

    fn delete(&self, folder_id: &str) -> Result<(bool, u32), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "folder store lock poisoned")?;
        if state.folders.remove(folder_id).is_none() {
            return Ok((false, 0));
        }
        let released = state
            .assignments
            .values()
            .filter(|id| id.as_str() == folder_id)
            .count() as u32;
        state.assignments.retain(|_, id| id.as_str() != folder_id);
        let mut folders: Vec<_> = state.folders.values_mut().collect();
        folders.sort_by_key(|folder| (folder.sort_order, folder.id.clone()));
        for (index, folder) in folders.into_iter().enumerate() {
            folder.sort_order = index as i32;
        }
        Ok((true, released))
    }

    fn assign(&self, session_id: &str, folder_id: Option<&str>) -> Result<Option<String>, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "folder store lock poisoned")?;
        if !state.sessions.contains_key(session_id) {
            return Err("session not found".into());
        }
        if let Some(folder_id) = folder_id {
            if !state.folders.contains_key(folder_id) {
                return Err("folder not found".into());
            }
        }
        let previous = state.assignments.remove(session_id);
        if let Some(folder_id) = folder_id {
            state
                .assignments
                .insert(session_id.into(), folder_id.into());
        }
        Ok(previous)
    }
}

struct NoopNotifier;
impl SessionFolderNotifier for NoopNotifier {
    fn publish(&self, _params: SessionFolderChangedParams) -> Result<(), String> {
        Ok(())
    }
}

pub struct SessionFolderHandler {
    store: Arc<dyn FolderStore>,
    notifier: Arc<dyn SessionFolderNotifier>,
}

impl SessionFolderHandler {
    pub fn new() -> Self {
        Self::with_dependencies(Arc::new(MemoryFolderStore::new()), Arc::new(NoopNotifier))
    }

    pub fn with_dependencies(
        store: Arc<dyn FolderStore>,
        notifier: Arc<dyn SessionFolderNotifier>,
    ) -> Self {
        Self { store, notifier }
    }

    fn internal(error: impl std::fmt::Display) -> ExtensionError {
        ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(error.to_string())),
        }
    }

    fn parse<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
        if !params.is_object() {
            return Err(ExtensionError::invalid_params("params must be an object"));
        }
        serde_json::from_value(params)
            .map_err(|error| ExtensionError::invalid_params(error.to_string()))
    }

    fn name(name: &str) -> Result<(), ExtensionError> {
        if name.trim().is_empty() || name.chars().count() > 100 {
            Err(ExtensionError::invalid_params("invalid name"))
        } else {
            Ok(())
        }
    }

    fn color(color: &str) -> Result<(), ExtensionError> {
        if color.len() != 7
            || !color.starts_with('#')
            || !color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            Err(ExtensionError::invalid_params("invalid color"))
        } else {
            Ok(())
        }
    }

    fn order(order: Option<i32>) -> Result<(), ExtensionError> {
        if order.is_some_and(|value| !(0..=MAX_SORT_ORDER).contains(&value)) {
            Err(ExtensionError::invalid_params("invalid sortOrder"))
        } else {
            Ok(())
        }
    }

    fn publish(&self, change: SessionFolderChange, folder_id: Option<String>) {
        let _ = self
            .notifier
            .publish(SessionFolderChangedParams { change, folder_id });
    }

    fn cursor(request: &SessionFolderListRequest) -> Result<Option<(i32, String)>, ExtensionError> {
        let pagination = PaginationParams {
            cursor: request.cursor.clone(),
            limit: request.limit,
        };
        let cursor = pagination.decode_cursor::<StableFolderCursor>()?;
        match cursor {
            None => Ok(None),
            Some(cursor) if cursor.order == ORDER_VERSION => {
                if cursor.folder_id.trim().is_empty()
                    || !(0..=MAX_SORT_ORDER).contains(&cursor.sort_order)
                {
                    Err(ExtensionError::invalid_params("invalid cursor"))
                } else {
                    Ok(Some((cursor.sort_order, cursor.folder_id)))
                }
            }
            Some(_) => Err(ExtensionError::invalid_params("invalid cursor ordering")),
        }
    }
}

impl Default for SessionFolderHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExtensionHandler for SessionFolderHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        _ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => {
                let request = if params.is_null() {
                    SessionFolderListRequest {
                        cursor: None,
                        limit: None,
                    }
                } else {
                    Self::parse(params)?
                };
                let pagination = PaginationParams {
                    cursor: request.cursor.clone(),
                    limit: request.limit,
                };
                let limit = pagination.limit_or_default(DEFAULT_LIMIT, MAX_LIMIT);
                if limit == 0 {
                    return Err(ExtensionError::invalid_params(
                        "limit must be greater than zero",
                    ));
                }
                let cursor = Self::cursor(&request)?;
                let snapshot = self.store.snapshot().map_err(Self::internal)?;
                let start = cursor
                    .as_ref()
                    .map(|(sort_order, folder_id)| {
                        snapshot
                            .folders
                            .iter()
                            .position(|folder| {
                                (folder.sort_order, folder.id.as_str())
                                    > (*sort_order, folder_id.as_str())
                            })
                            .unwrap_or(snapshot.folders.len())
                    })
                    .unwrap_or(0);
                let end = start.saturating_add(limit).min(snapshot.folders.len());
                let items = snapshot.folders[start..end].to_vec();
                let has_more = end < snapshot.folders.len();
                let next_cursor = has_more.then(|| {
                    let folder = &items[items.len() - 1];
                    encode_cursor(serde_json::json!({
                        "sortOrder": folder.sort_order,
                        "folderId": folder.id,
                        "order": ORDER_VERSION,
                    }))
                });
                Ok(serde_json::to_value(SessionFolderListResponse {
                    items,
                    unassigned_count: snapshot.unassigned_count,
                    next_cursor,
                    has_more,
                })
                .map_err(Self::internal)?)
            }
            "create" => {
                let request: SessionFolderCreateRequest = Self::parse(params)?;
                Self::name(&request.name)?;
                Self::order(request.sort_order)?;
                if let Some(color) = &request.color {
                    Self::color(color)?;
                }
                let folder = self
                    .store
                    .create(request.name, request.color, request.sort_order)
                    .map_err(Self::internal)?;
                self.publish(SessionFolderChange::Create, Some(folder.id.clone()));
                serde_json::to_value(SessionFolderCreateResponse { folder }).map_err(Self::internal)
            }
            "update" => {
                let request: SessionFolderUpdateRequest = Self::parse(params)?;
                if request.folder_id.trim().is_empty() {
                    return Err(ExtensionError::invalid_params("folderId must not be empty"));
                }
                if let Some(name) = &request.name {
                    Self::name(name)?;
                }
                if let Some(Some(color)) = &request.color {
                    Self::color(color)?;
                }
                Self::order(request.sort_order)?;
                let folder = self
                    .store
                    .update(
                        &request.folder_id,
                        request.name,
                        request.color,
                        request.sort_order,
                    )
                    .map_err(|error| {
                        if error == "folder not found" {
                            ExtensionError::invalid_params("folderId does not exist")
                        } else {
                            Self::internal(error)
                        }
                    })?;
                self.publish(SessionFolderChange::Update, Some(folder.id.clone()));
                serde_json::to_value(SessionFolderUpdateResponse {
                    updated: true,
                    folder,
                })
                .map_err(Self::internal)
            }
            "delete" => {
                let request: SessionFolderDeleteRequest = Self::parse(params)?;
                if request.folder_id.trim().is_empty() {
                    return Err(ExtensionError::invalid_params("folderId must not be empty"));
                }
                let (deleted, released_sessions) = self
                    .store
                    .delete(&request.folder_id)
                    .map_err(Self::internal)?;
                if deleted {
                    self.publish(SessionFolderChange::Delete, Some(request.folder_id.clone()));
                }
                serde_json::to_value(SessionFolderDeleteResponse {
                    deleted,
                    folder_id: request.folder_id,
                    released_sessions,
                })
                .map_err(Self::internal)
            }
            "assign" => {
                if !params.is_object() || params.get("folderId").is_none() {
                    return Err(ExtensionError::invalid_params("folderId is required"));
                }
                let request: SessionFolderAssignRequest = Self::parse(params)?;
                if request.session_id.trim().is_empty() {
                    return Err(ExtensionError::invalid_params(
                        "sessionId must not be empty",
                    ));
                }
                if request
                    .folder_id
                    .as_deref()
                    .is_some_and(|id| id.trim().is_empty())
                {
                    return Err(ExtensionError::invalid_params("folderId must not be empty"));
                }
                let previous = self
                    .store
                    .assign(&request.session_id, request.folder_id.as_deref())
                    .map_err(|error| match error.as_str() {
                        "session not found" | "folder not found" => {
                            ExtensionError::invalid_params(error)
                        }
                        _ => Self::internal(error),
                    })?;
                self.publish(SessionFolderChange::Assign, request.folder_id.clone());
                serde_json::to_value(SessionFolderAssignResponse {
                    assigned: true,
                    session_id: request.session_id,
                    folder_id: request.folder_id,
                    previous_folder_id: previous,
                })
                .map_err(Self::internal)
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({"list": true, "create": true, "update": true, "delete": true, "assign": true})
    }
}
