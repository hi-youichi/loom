//! Experimental endpoint group (task P2.21).
//!
//! TUI hits `/experimental/*` for capability discovery and resource
//! listing. We return empty placeholders so the bootstrap resolves; a
//! follow-up PR can wire real sub-routes when loom has actual
//! corresponding features.
//!
//! ## Worktree lifecycle removed (W4 cleanup)
//!
//! opencode has NO worktree group. The previous worktree lifecycle
//! handlers (`/worktree`, `/experimental/worktree/*`) were removed in
//! task W4 along with their routes. This server reports the active
//! directory as informational metadata only — it does not manage a git
//! worktree lifecycle.

use axum::{extract::Path, Json};
use serde_json::{json, Value};

/// `GET /experimental/capabilities` — list enabled features.
pub async fn get_capabilities() -> Json<Value> {
    Json(json!({
        "backgroundSubagents": true,
        // Compatibility flags used by rollout-v2 clients.
        "agents": true,
        "tools": true,
        "mcp": true,
        "permissions": false,
        "questions": false,
        "sessions": true,
        "experimentalTools": false,
    }))
}

/// `GET /experimental/console` — TUI bootstrap state.
///
/// The v2 SDK contract (`packages/tui/src/context/sync.tsx:458`) requires
/// the response body to match `ConsoleState`:
/// ```ts
/// type ConsoleState = {
///   consoleManagedProviders: string[]   // provider ids managed by the console
///   switchableOrgCount: number          // number of orgs the user can switch to
/// }
/// ```
///
/// **Response shape contract**: the v2 SDK (`@opencode-ai/sdk/v2`) wraps the
/// parsed JSON body in `{data, request, response}` (see `client.gen.js:150`)
/// and the TUI calls `.then((x) => x.data)` to unwrap. **The server must
/// return the raw `ConsoleState` directly** — wrapping in `{"data": {...}}`
/// causes the TUI to store `console_state = {data: {...}}`, after which
/// `sync.data.console_state.consoleManagedProviders` is `undefined` and
/// `dialog-provider.tsx:135` (`isConsoleManagedProvider(undefined, ...)`)
/// crashes the TUI with
/// `undefined is not an object (evaluating 'consoleManagedProviders.has')`
/// (opencode bug-report "U.has" pattern). We return safe empty defaults so
/// the bootstrap completes; the `endpoint` field is preserved for any future
/// SSE registration follow-up.
pub async fn get_console() -> Json<Value> {
    Json(json!({
        "consoleManagedProviders": [],
        "switchableOrgCount": 0,
        "endpoint": "/experimental/console",
    }))
}

/// `GET /experimental/console/orgs` — org list (always empty in MVP).
pub async fn get_console_orgs() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `POST /experimental/console/org` — create an org (stub).
pub async fn post_console_org(Json(_body): Json<Value>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// `GET /experimental/resource` — list resources.
pub async fn get_resource() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `GET /experimental/resource/list` — same.
pub async fn get_resource_list() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `POST /experimental/resource` — create a resource.
pub async fn post_resource(Json(_body): Json<Value>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// `GET /experimental/resource/:id` — get one.
pub async fn get_resource_one(Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "id": id }))
}

/// `DELETE /experimental/resource/:id` — delete one.
pub async fn delete_resource_one(Path(_id): Path<String>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// `POST /experimental/eval` — evaluate a snippet. Stub.
pub async fn post_eval(Json(body): Json<Value>) -> Json<Value> {
    Json(json!({ "result": body.get("input").cloned().unwrap_or(json!(null)) }))
}

#[cfg(test)]
mod tests {
    //! Regression tests for the `consoleManagedProviders.has` TUI crash
    //! (opencode bug report URL pattern: undefined is not an object
    //! (evaluating 'consoleManagedProviders.has')).
    //!
    //! The TUI's `packages/tui/src/context/sync.tsx:458` calls
    //! `sdk.client.experimental.console.get(...)` which already wraps the
    //! response body in `{data, request, response}` (see SDK
    //! `client.gen.js:150` — `responseStyle` default `"data"`) and then
    //! `.then((x) => x.data)` unwraps. So the server must return the
    //! raw `ConsoleState` directly — *not* wrapped in `{"data": ...}`.
    //! Returning the envelope causes `console_state` to be `{data: {...}}`,
    //! `sync.data.console_state.consoleManagedProviders` is `undefined`,
    //! and `dialog-provider.tsx:135` (`isConsoleManagedProvider(undefined)`)
    //! crashes with the `.has(...)` TypeError.

    use super::*;
    use serde_json::Value;

    #[tokio::test]
    async fn get_console_returns_raw_console_state_not_envelope() {
        let Json(payload) = get_console().await;

        // Top-level `consoleManagedProviders` must exist (no `data`
        // wrapper). Returning `{"data": {...}}` here would cause the SDK
        // to unwrap once → `{...}`, then the TUI `.data`-unwrap on top
        // would surface `{"data": {...}}` to `reconcile()`, corrupting
        // the store.
        assert!(
            payload.get("consoleManagedProviders").is_some(),
            "top-level `consoleManagedProviders` must be present (got {payload})",
        );
        assert!(
            payload["consoleManagedProviders"].is_array(),
            "`consoleManagedProviders` must be a JSON array (got {})",
            payload["consoleManagedProviders"],
        );
        assert!(
            payload.get("switchableOrgCount").is_some(),
            "top-level `switchableOrgCount` must be present (got {payload})",
        );
        assert!(
            payload["switchableOrgCount"].is_number(),
            "`switchableOrgCount` must be a JSON number (got {})",
            payload["switchableOrgCount"],
        );

        // Optional field for SSE registration follow-up.
        assert_eq!(payload["endpoint"], "/experimental/console");
    }

    #[test]
    fn empty_console_managed_providers_is_a_real_array_not_undefined() {
        // The TUI's `Array.isArray(...)` check in `provider-origin.ts:4`
        // discriminates on this shape. A bare `null` or missing field
        // routes into the `.has(...)` branch and crashes the TUI.
        let Json(payload) = futures::executor::block_on(get_console());
        let Value::Array(arr) = &payload["consoleManagedProviders"] else {
            panic!(
                "consoleManagedProviders must serialize as a JSON array at the \
                 top level (the v2 SDK already wraps in {{data, ...}} — double \
                 wrapping makes the field undefined inside the TUI store). \
                 Got: {}",
                payload["consoleManagedProviders"],
            );
        };
        assert!(arr.is_empty(), "MVP has no console-managed providers");
    }

    #[test]
    fn get_console_does_not_double_envelope_with_data_field() {
        // Regression: an earlier version of this handler returned
        // `{"data": {...}}` and the TUI crashed with the bug-report
        // stack. Lock the contract: the body must NOT have a top-level
        // `data` key wrapping the actual ConsoleState fields.
        let Json(payload) = futures::executor::block_on(get_console());
        assert!(
            payload.get("data").is_none(),
            "handler must NOT wrap in {{data: ...}} — the v2 SDK already \
             wraps; double-wrapping causes `console_state.consoleManagedProviders` \
             to be undefined inside the TUI. Got: {payload}",
        );
    }
}
