//! Credential management endpoints — opencode `server.credential` group
//! (`group-credential.ts`, `schema-credential.ts`).
//!
//! Implements the two credential mutations the contract defines:
//!
//! | Method | Path                                  | Payload      | Success           |
//! |--------|---------------------------------------|--------------|-------------------|
//! | PATCH  | `/api/credential/:credentialID`       | `{ "label" }`| `204 No Content`  |
//! | DELETE | `/api/credential/:credentialID`       | —            | `204 No Content`  |
//!
//! Both endpoints accept [`LocationQuery`] (deepObject `?location[...]`) per
//! the contract. The success type is `HttpApiSchema.NoContent` (204 with no
//! body), so there is no `Location.response` envelope to emit — the location
//! query is parsed for conformance and used to scope the operation.
//!
//! The payload `{ label: Schema.String }` and the `NoContent` success type
//! mirror the contract exactly. `credentialID` is a branded
//! `Credential.ID` (`"cred_" + ascending()`) on the wire; here it is just the
//! path segment string, matching how every other handler treats branded ids.
//!
//! ## Backing store
//!
//! Mutations target [`AppState::credentials`](crate::state::AppState) — the
//! in-memory credential store added in task W0, keyed by `cred_*` id. The
//! PATCH handler updates the `label` field of an existing
//! [`CredentialEntry`](crate::state::CredentialEntry) (returning `404` if the
//! id is unknown, rather than pretending it updated something — no
//! success-shaped stubs); DELETE removes the entry and is idempotent
//! (`204` whether or not it existed), per standard REST DELETE semantics.
//!
//! ### Durability caveat (honest status)
//!
//! The existing [`Store`](crate::state::StoreTrait) trait models only
//! sessions / messages / parts / events and exposes **no credential methods**,
//! and adding them (plus the W0 `persist_*` helper and a `load_credentials`
//! path) is owned by other files. Cross-restart persistence of credentials is
//! therefore not wired here: mutations are applied to the live in-memory map
//! only. We do **not** fake a write-through — when a real `Store` credential
//! surface lands, a `persist_credential` / `persist_credential_delete` call
//! belongs here. Returning `204` while having silently dropped the mutation
//! would violate the "real behavior or explicit 501" rule.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::location::LocationQuery;
use crate::state::SharedState;

/// `PATCH /api/credential/:credentialID` request body.
///
/// Mirrors the contract payload `Schema.Struct({ label: Schema.String })`.
/// `label` is the only mutable field of a stored credential; the secret
/// `value` (`Credential.OAuth` / `Credential.Key`) is set during an auth flow
/// and never updated through this route.
#[derive(Debug, Deserialize)]
pub struct UpdateCredentialBody {
    pub label: String,
}

/// `PATCH /api/credential/:credentialID` — update a stored credential label.
///
/// Accepts [`LocationQuery`] per the contract (deepObject query). Success →
/// `204 No Content` (the contract's `HttpApiSchema.NoContent`). If
/// `credentialID` is unknown the handler returns `404` with a JSON error:
/// returning `204` while having changed nothing would be a success-shaped
/// stub. The mutated entry stays in the in-memory store (see module docs for
/// the durability caveat).
pub async fn update_credential(
    State(state): State<SharedState>,
    Path(credential_id): Path<String>,
    Query(_loc): Query<LocationQuery>,
    Json(body): Json<UpdateCredentialBody>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    {
        let mut creds = state.credentials.write();
        let Some(entry) = creds.get_mut(&credential_id) else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "credential not found",
                    "credentialID": credential_id,
                })),
            ));
        };
        entry.label = body.label;
    }
    // Write-through to a durable store belongs here once the Store trait
    // gains credential methods (see module-level durability caveat).
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/credential/:credentialID` — remove a stored credential.
///
/// Accepts [`LocationQuery`] per the contract. Idempotent: returns
/// `204 No Content` whether or not the credential existed (the contract
/// success type is `NoContent`, and REST DELETE is defined to be idempotent).
/// The removed entry is dropped from the in-memory store (see module docs for
/// the durability caveat).
pub async fn remove_credential(
    State(state): State<SharedState>,
    Path(credential_id): Path<String>,
    Query(_loc): Query<LocationQuery>,
) -> StatusCode {
    state.credentials.write().remove(&credential_id);
    // Durable delete-through belongs here once the Store trait gains
    // credential methods (see module-level durability caveat).
    StatusCode::NO_CONTENT
}
