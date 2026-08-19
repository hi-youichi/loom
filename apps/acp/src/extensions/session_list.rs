//! `_loomdesk.dev/session/*` — global session listing + archival.
//!
//! Replaces the legacy Express→opencode `/api/experimental/session`
//! passthrough: the sidebar pulls active and archived sessions from the
//! Loom-owned `acp_sessions` table via `list-global`, and archive/unarchive
//! mutations are persisted server-side and broadcast as `session.updated`
//! global events so every connected client stays in sync.
//!
//! Spec: docs/acp-spec/extensions/39-session-list.md

use std::sync::{Arc, RwLock, Weak};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent::LoomAcpAgent;
use crate::extensions::pagination::{encode_cursor, PaginationParams};
use crate::extensions::{ExtensionContext, ExtensionError, ExtensionHandler};
use crate::global_events::GlobalEventBus;
use crate::session_repository::SessionMetadata;

pub const DOMAIN: &str = "session";

const DEFAULT_PAGE_LIMIT: usize = 200;
const MAX_PAGE_LIMIT: usize = 1000;

/// Handler for the `session` domain. The agent reference is late-bound
/// (registry registration happens before agent construction).
pub struct SessionListHandler {
    agent: RwLock<Option<Weak<LoomAcpAgent>>>,
    global_bus: Option<Arc<GlobalEventBus>>,
}

impl SessionListHandler {
    pub fn new() -> Self {
        Self {
            agent: RwLock::new(None),
            global_bus: None,
        }
    }

    pub fn with_global_bus(mut self, bus: Arc<GlobalEventBus>) -> Self {
        self.global_bus = Some(bus);
        self
    }

    pub fn bind(&self, agent: &Arc<LoomAcpAgent>) {
        *self.agent.write().expect("session-list bind poisoned") = Some(Arc::downgrade(agent));
    }

    fn agent(&self) -> Result<Arc<LoomAcpAgent>, ExtensionError> {
        self.agent
            .read()
            .expect("session-list agent poisoned")
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or_else(|| ExtensionError {
                code: -32603,
                message: "internal_error".into(),
                data: Some(Value::String("agent not bound".into())),
            })
    }

    async fn handle_list_global(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let parsed: ListGlobalParams = serde_json::from_value(params)
            .map_err(|error| ExtensionError::invalid_params(format!("{error}")))?;
        let before_updated_at = parsed
            .pagination
            .decode_cursor::<String>()
            .map_err(|error| ExtensionError::invalid_params(format!("{error}")))?;
        let limit = parsed.pagination.limit_or_default(DEFAULT_PAGE_LIMIT, MAX_PAGE_LIMIT);

        let agent = self.agent()?;
        let (rows, next_cursor) = agent
            .list_global_sessions_for_owner(
                &ctx.principal,
                parsed.archived,
                parsed.directory.as_deref(),
                limit,
                before_updated_at.as_deref(),
            )
            .await
            .map_err(|error| internal_error(error.message))?;

        let sessions: Vec<Value> = rows.iter().map(to_list_item).collect();
        let has_more = next_cursor.is_some();
        let next_cursor = next_cursor.map(|cursor| encode_cursor(Value::String(cursor)));
        Ok(json!({
            "sessions": sessions,
            "nextCursor": next_cursor,
            "hasMore": has_more,
        }))
    }

    async fn handle_archive(
        &self,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        let parsed: ArchiveParams = serde_json::from_value(params)
            .map_err(|error| ExtensionError::invalid_params(format!("{error}")))?;
        if parsed.session_id.is_empty() {
            return Err(ExtensionError::invalid_params("sessionId is required"));
        }

        let agent = self.agent()?;
        let updated = agent
            .archive_session_for_owner(&ctx.principal, &parsed.session_id, parsed.archived)
            .await
            .map_err(|error| internal_error(error.message))?
            .ok_or_else(|| ExtensionError::not_found("session not found"))?;

        if let Some(bus) = &self.global_bus {
            bus.publish(
                "session",
                "session.updated",
                json!({ "info": to_event_info(&updated) }),
            );
        }

        Ok(json!({ "session": to_list_item(&updated) }))
    }
}

impl Default for SessionListHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ListGlobalParams {
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    directory: Option<String>,
    #[serde(flatten)]
    pagination: PaginationParams,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveParams {
    session_id: String,
    #[serde(default = "default_archived")]
    archived: bool,
}

fn default_archived() -> bool {
    true
}

fn internal_error(message: impl Into<String>) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(message.into())),
    }
}

/// Loom-native list item consumed by the FE adapter
/// (`acpSessionInfoToOpenCodeSession`-style mapping).
fn to_list_item(metadata: &SessionMetadata) -> Value {
    json!({
        "sessionId": metadata.session_id,
        "cwd": metadata.cwd.to_string_lossy(),
        "title": metadata.title.clone().unwrap_or_default(),
        "createdAt": metadata.created_at,
        "updatedAt": metadata.updated_at,
        "archivedAt": metadata.archived_at,
    })
}

/// opencode-shaped session info for `session.updated` global events; the FE
/// event reducer requires `id` plus `time` (with `time.archived` for
/// archival transitions).
fn to_event_info(metadata: &SessionMetadata) -> Value {
    let cwd = metadata.cwd.to_string_lossy();
    let directory = strip_verbatim_prefix(&cwd);
    json!({
        "id": metadata.session_id,
        "title": metadata.title.clone().unwrap_or_default(),
        "parentID": "",
        "project": directory,
        "directory": directory,
        "time": {
            "created": metadata.created_at.as_deref().and_then(rfc3339_to_ms).unwrap_or(0),
            "updated": metadata
                .updated_at
                .as_deref()
                .and_then(rfc3339_to_ms)
                .or_else(|| metadata.created_at.as_deref().and_then(rfc3339_to_ms))
                .unwrap_or(0),
            "archived": metadata.archived_at.as_deref().and_then(rfc3339_to_ms),
        },
        "share": Value::Null,
        "version": "",
        "replay": Value::Null,
    })
}

fn strip_verbatim_prefix(path: &str) -> String {
    path.strip_prefix(r"\\?\")
        .unwrap_or(path)
        .replace('\\', "/")
}

fn rfc3339_to_ms(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

#[async_trait]
impl ExtensionHandler for SessionListHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list-global" => self.handle_list_global(params, ctx).await,
            "archive" => self.handle_archive(params, ctx).await,
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        json!({ "methods": ["list-global", "archive"] })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_info_maps_opencode_shape() {
        let metadata = SessionMetadata {
            session_id: "s-1".into(),
            thread_id: "t-1".into(),
            owner_principal: "local-anonymous".into(),
            cwd: r"C:\repo".into(),
            lifecycle: "idle".into(),
            title: Some("Title".into()),
            updated_at: Some("2026-08-18T10:00:00+00:00".into()),
            created_at: Some("2026-08-18T09:00:00+00:00".into()),
            archived_at: Some("2026-08-18T11:00:00+00:00".into()),
        };
        let info = to_event_info(&metadata);
        assert_eq!(info["id"], "s-1");
        assert_eq!(info["directory"], "C:/repo");
        assert_eq!(info["time"]["created"], 1_787_043_600_000i64);
        assert_eq!(info["time"]["updated"], 1_787_047_200_000i64);
        assert_eq!(info["time"]["archived"], 1_787_050_800_000i64);
    }

    #[test]
    fn unarchived_info_has_null_archived() {
        let metadata = SessionMetadata {
            archived_at: None,
            created_at: None,
            updated_at: None,
            ..base_metadata()
        };
        let info = to_event_info(&metadata);
        assert!(info["time"]["archived"].is_null());
    }

    fn base_metadata() -> SessionMetadata {
        SessionMetadata {
            session_id: "s".into(),
            thread_id: "t".into(),
            owner_principal: "local-anonymous".into(),
            cwd: r"C:\repo".into(),
            lifecycle: "idle".into(),
            title: None,
            updated_at: None,
            created_at: None,
            archived_at: None,
        }
    }
}
