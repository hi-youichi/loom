//! opencode `Location` conformance primitives.
//!
//! Implements the contract in `.loom/opencode-ref/schema/location.ts` and
//! `.loom/opencode-ref/protocol/groups/location.ts`:
//!
//! - [`LocationInfo`] mirrors `Location.Info` = `{ directory, workspaceID?,
//!   project: { id, directory } }` — the closed struct returned by
//!   `GET /api/location` and embedded in every `Location.response` envelope.
//! - [`LocationQuery`] is an axum extractor that decodes the deepObject query
//!   `?location[directory]=..&location[workspace]=..` (OpenAPI
//!   `style: deepObject, explode: true`) and defaults to the server project
//!   directory when the params are absent.
//! - [`location_response`] wraps any `Serialize` payload as
//!   `{ location: Location.Info, data }` — the `Location.response` envelope
//!   (schema/location.ts:23-25) every list/get endpoint returns.
//!
//! loom-server operates on a single location (the cwd) tracked by
//! [`AppState::project`](crate::state::AppState), so the location is always
//! resolved from there. The query params are accepted for conformance and
//! exposed for handlers that want the requested values.

use axum::{
    async_trait,
    extract::{FromRequestParts, Query},
    http::request::Parts,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::state::SharedState;

/// `Location.Info` (schema/location.ts:14-21). Closed struct: `directory`
/// is required, `workspaceID` is optional, `project` is `{ id, directory }`.
///
/// Serialized field names match the opencode contract verbatim — notably
/// `workspaceID` (camelCase), not `workspace_id`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocationInfo {
    pub directory: String,
    #[serde(rename = "workspaceID", skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub project: LocationProject,
}

/// Nested `project` member of [`LocationInfo`] (schema/location.ts:17-20).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocationProject {
    pub id: String,
    pub directory: String,
}

impl LocationInfo {
    /// Resolve `Location.Info` from the server's active project — the single
    /// location loom-server operates on. Used by `GET /api/location` and by
    /// [`location_response`] to populate every `Location.response` envelope.
    pub fn from_state(state: &SharedState) -> Self {
        let project = state.project.read();
        Self {
            directory: project.directory.clone(),
            workspace_id: project.workspace_id.clone(),
            project: LocationProject {
                id: project.id.clone(),
                directory: project.directory.clone(),
            },
        }
    }
}

/// `LocationQuery` (protocol/groups/location.ts:5-12): an optional deepObject
/// `location` with `directory` / `workspace` members.
///
/// The opencode contract encodes this as
/// `?location[directory]=..&location[workspace]=..` (OpenAPI
/// `style: deepObject, explode: true`). Because `serde_urlencoded`
/// percent-decodes keys but treats `location[directory]` as a single opaque
/// key (it does not interpret brackets), renaming the flat fields to the
/// bracketed keys parses the deepObject form directly.
///
/// Both fields are optional; `#[serde(default)]` makes a fully-absent query
/// deserialize cleanly (handlers then fall back to the server project dir via
/// [`LocationQuery::resolve_directory`]).
///
/// Usable either as a first-class extractor (`loc: LocationQuery`) or wrapped
/// in axum's query extractor (`Query<LocationQuery>`) — both honor the
/// deepObject form.
#[derive(Deserialize, Default, Clone)]
pub struct LocationQuery {
    #[serde(default, rename = "location[directory]")]
    pub directory: Option<String>,
    #[serde(default, rename = "location[workspace]")]
    pub workspace: Option<String>,
}

impl LocationQuery {
    /// Resolve the effective directory: the one the client requested via
    /// `?location[directory]=`, or the server's active project directory when
    /// the client did not supply one.
    pub fn resolve_directory(&self, state: &SharedState) -> String {
        if let Some(dir) = self.directory.clone() {
            return dir;
        }
        state.project.read().directory.clone()
    }
}

// First-class axum extractor: handlers can write `loc: LocationQuery` directly.
// Delegates to axum's `Query` (which owns percent-decoding + serde_urlencoded
// parsing); falls back to an empty query on any parse failure because the
// whole shape is optional (an unparseable query means "no location requested").
#[async_trait]
impl<S> FromRequestParts<S> for LocationQuery
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Ok(Query::<LocationQuery>::from_request_parts(parts, state)
            .await
            .map(|Query(loc)| loc)
            .unwrap_or_default())
    }
}

/// Wrap `data` in the `Location.response` envelope (schema/location.ts:23-25):
/// `{ location: Location.Info, data }`. The `location` is resolved from the
/// server's active project via [`LocationInfo::from_state`].
///
/// Generic over any `Serialize` payload so every list/get handler can call
/// `location_response(&state, payload)` regardless of its concrete schema.
pub fn location_response<T>(state: &SharedState, data: T) -> Json<Value>
where
    T: Serialize,
{
    let location = serde_json::to_value(LocationInfo::from_state(state)).unwrap_or(Value::Null);
    let data = serde_json::to_value(&data).unwrap_or(Value::Null);
    Json(json!({
        "location": location,
        "data": data,
    }))
}
