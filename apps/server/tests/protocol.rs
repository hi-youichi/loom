//! Integration tests for the loom-server route surface.
//!
//! Strategy: build a Router+State pair in each test, then drive it via
//! `axum::Router::oneshot`. State is shared across all `oneshot` calls
//! in the same test (so PATCH/POST then GET sees persisted changes),
//! but a fresh state is built per test.
//!
//! These tests correspond to task P3.25 — every URL defined in
//! `routes.rs` should resolve to either 2xx or 4xx without disconnecting.
//! Failure modes here usually mean a route isn't registered, a
//! path-compat issue (`/api/...` vs `/...`), or a missing extractor type.

use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use futures::StreamExt;
use loom_server::routes::build_router;
use loom_server::state::{new_state, SharedState};
use serde_json::Value;
use tower::ServiceExt;

const MAX_BODY: usize = 1024 * 64;

/// Build a Router+State pair that share state across `oneshot` calls.
/// Returns the shared state for direct inspection plus the router to
/// drive requests through.
fn router_with_state() -> (SharedState, axum::Router) {
    let state = new_state();
    let router = build_router(state.clone());
    (state, router)
}

async fn json_get(router: axum::Router, path: &str) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    let status = response.status();
    let body = to_bytes(response.into_body(), MAX_BODY)
        .await
        .unwrap_or_default();
    let json = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    (status, json)
}

async fn json_post(router: axum::Router, path: &str, body: Value) -> (StatusCode, Value) {
    let body_str = serde_json::to_string(&body).unwrap();
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body_str))
                .unwrap(),
        )
        .await
        .expect("request");
    let status = response.status();
    let body = to_bytes(response.into_body(), MAX_BODY)
        .await
        .unwrap_or_default();
    let json = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    (status, json)
}

async fn patch(router: axum::Router, path: &str, body: Value) -> (StatusCode, Value) {
    let body_str = serde_json::to_string(&body).unwrap();
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body_str))
                .unwrap(),
        )
        .await
        .expect("request");
    let status = response.status();
    let body = to_bytes(response.into_body(), MAX_BODY)
        .await
        .unwrap_or_default();
    let json = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    (status, json)
}

async fn delete(router: axum::Router, path: &str) -> StatusCode {
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    response.status()
}

// === BEHAVIOR vs STUB/ROUTE-COVERAGE TESTS (LS-001) ===========================
// Every test below is tagged with its classification so it is clear which
// tests back implemented features vs. which only prove a route exists.
//
//   // [BEHAVIOR]            — verifies a real state change, a round-trip
//                             (mutate then read back), an explicit error
//                             path (404/501), or an actually executed
//                             operation (shell, SSE replay, fork/copy).
//                             === BEHAVIOR TESTS ===
//
//   // [STUB/ROUTE-COVERAGE] — only asserts the route is registered and
//                             returns 2xx with the expected envelope shape
//                             (is_object / is_array / data-is-present / ok).
//                             The handler behind it is a stub or a fixed
//                             placeholder; no real behavior is verified.
//                             === STUB/ROUTE-COVERAGE TESTS ===
//
// Tests are NOT reordered — they keep their original area sections — but each
// carries its LS-001 tag so the two groups above are greppable per test.
// === (end LS-001 classification legend) ========================================

// ───────── bootstrap (P0.2, P0.3) ─────────────────────────────────

// [STUB/ROUTE-COVERAGE] only checks 2xx + is_object shape; no real behavior.
#[tokio::test]
async fn bootstrap_v1_get_config_returns_object() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/config").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.is_object());
}

// [STUB/ROUTE-COVERAGE] only checks 2xx + is_object shape; no real behavior.
#[tokio::test]
async fn bootstrap_v2_get_api_config_returns_object() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/config").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.is_object());
}

// [BEHAVIOR] PATCH persists a value that a subsequent GET reads back.
#[tokio::test]
async fn bootstrap_patch_api_config_round_trip() {
    let (_, router) = router_with_state();
    // PATCH and GET share the same router → same state.
    let (s, _) = patch(
        router.clone(),
        "/api/config",
        serde_json::json!({"theme": "dark"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, after) = json_get(router, "/api/config").await;
    assert_eq!(after["theme"], "dark");
}

// [STUB/ROUTE-COVERAGE] envelope shape of the fixed provider registry only.
#[tokio::test]
async fn bootstrap_provider_list_matches_v1_sdk_shape() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/provider").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["all"].is_array());
    assert!(body["default"].is_object());
    assert!(body["connected"].is_array());
}

// [BEHAVIOR] asserts the real "build" agent is present in the registry.
#[tokio::test]
async fn bootstrap_agent_list_includes_build_agent() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/agent").await;
    assert_eq!(s, StatusCode::OK);
    let agents = body.as_array().cloned().unwrap_or_default();
    assert!(!agents.is_empty(), "build agent must be discoverable");
    assert_eq!(agents[0]["name"], "build");
}

// [STUB/ROUTE-COVERAGE] /lsp/status is an empty-array stub; shape only.
#[tokio::test]
async fn bootstrap_lsp_status_is_array() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/lsp/status").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.is_array());
}

// [STUB/ROUTE-COVERAGE] /formatter/status is an empty-array stub; shape only.
#[tokio::test]
async fn bootstrap_formatter_status_is_array() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/formatter/status").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.is_array());
}

// [STUB/ROUTE-COVERAGE] only checks 2xx + is_object shape; no real behavior.
#[tokio::test]
async fn bootstrap_session_status_is_session_map() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/session/status").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.is_object());
}

// [STUB/ROUTE-COVERAGE] checks one fixed capability flag; shape only.
#[tokio::test]
async fn bootstrap_experimental_capabilities_matches_current_sdk() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/experimental/capabilities").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["backgroundSubagents"], true);
}

// [STUB/ROUTE-COVERAGE] health envelope shape (ok:true) only.
#[tokio::test]
async fn bootstrap_v2_api_health_ok() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/health").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["healthy"], true);
}

// [STUB/ROUTE-COVERAGE] health envelope shape (ok:true) only.
#[tokio::test]
async fn bootstrap_global_health_ok() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/global/health").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

// [STUB/ROUTE-COVERAGE] GET-only; 2xx + id-is-string shape (no mutation).
#[tokio::test]
async fn bootstrap_v2_api_location_round_trip() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/location").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["project"]["id"].is_string());
}

// [STUB/ROUTE-COVERAGE] only checks 2xx + cwd-is-string shape.
#[tokio::test]
async fn bootstrap_v2_api_path_cwd() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/path").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["cwd"].is_string());
}

// [STUB/ROUTE-COVERAGE] only checks 2xx + data envelope is present.
#[tokio::test]
async fn bootstrap_v2_api_agent_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/agent").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// [STUB/ROUTE-COVERAGE] only checks 2xx + data envelope is present.
#[tokio::test]
async fn bootstrap_v2_api_command_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/command").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// [STUB/ROUTE-COVERAGE] only checks 2xx + data envelope is present.
#[tokio::test]
async fn bootstrap_v2_api_provider_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/provider").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// [STUB/ROUTE-COVERAGE] only checks 2xx + data envelope is present.
#[tokio::test]
async fn bootstrap_v2_api_model_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/model").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// [STUB/ROUTE-COVERAGE] only checks 2xx + data envelope is present.
#[tokio::test]
async fn bootstrap_v2_api_skill_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/skill").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// [STUB/ROUTE-COVERAGE] only checks 2xx + data envelope is present.
#[tokio::test]
async fn bootstrap_v2_api_reference_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/reference").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// [STUB/ROUTE-COVERAGE] only checks 2xx + data envelope is present.
#[tokio::test]
async fn bootstrap_v2_api_integration_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/integration").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// [STUB/ROUTE-COVERAGE] /vcs/status is a fixed clean-state stub; shape only.
#[tokio::test]
async fn bootstrap_v2_api_vcs_status() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/vcs/status").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["dirty"].is_boolean());
}

// [STUB/ROUTE-COVERAGE] /api/permission/saved is a stub; data shape only.
#[tokio::test]
async fn bootstrap_v2_api_permission_saved() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/permission/saved").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// ───────── session (P1.13, P1.14) ─────────────────────────────────

// [BEHAVIOR] POST creates a session and returns a real generated id.
#[tokio::test]
async fn session_create_returns_info_with_id() {
    let (_, router) = router_with_state();
    let (s, body) = json_post(router, "/session", serde_json::json!({})).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["id"].is_string());
}

// [BEHAVIOR] supplied directory persists into directory + path.cwd/root.
#[tokio::test]
async fn session_create_persists_directory_in_path_envelope() {
    let (_, router) = router_with_state();
    let directory = "C:/tmp/loom-test-project";
    let (s, body) = json_post(
        router,
        "/session",
        serde_json::json!({ "directory": directory }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["directory"], directory);
    assert_eq!(body["path"]["cwd"], directory);
    assert_eq!(body["path"]["root"], directory);
}

// [BEHAVIOR] verifies the 404 error path for an unknown session.
#[tokio::test]
async fn session_get_unknown_returns_404() {
    let (_, router) = router_with_state();
    let (s, _) = json_get(router, "/session/sess_unknown").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// [BEHAVIOR] verifies the 404 error path for deleting an unknown session.
#[tokio::test]
async fn session_delete_unknown_returns_404() {
    let (_, router) = router_with_state();
    let s = delete(router, "/session/sess_unknown").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// [BEHAVIOR] PATCH title persists and is observable in the response.
#[tokio::test]
async fn session_patch_title_round_trip() {
    let (_, router) = router_with_state();
    let (_, created) = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let id = created["id"].as_str().unwrap().to_string();
    let (s, after) = patch(
        router.clone(),
        &format!("/api/session/{id}"),
        serde_json::json!({"title": "Renamed"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(after["title"], "Renamed");
}

// [BEHAVIOR] PATCH directory persists + a follow-up GET reads it back.
#[tokio::test]
async fn session_patch_updates_directory_in_path_envelope() {
    let (_, router) = router_with_state();
    let (_, created) = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let id = created["id"].as_str().unwrap().to_string();
    let directory = "C:/tmp/loom-patched-project";
    let (s, after) = patch(
        router.clone(),
        &format!("/api/session/{id}"),
        serde_json::json!({ "directory": directory }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(after["directory"], directory);
    assert_eq!(after["path"]["cwd"], directory);
    assert_eq!(after["path"]["root"], directory);

    let (s, fetched) = json_get(router, &format!("/session/{id}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(fetched["directory"], directory);
    assert_eq!(fetched["path"]["cwd"], directory);
    assert_eq!(fetched["path"]["root"], directory);
}

// [STUB/ROUTE-COVERAGE] only checks 2xx + is_array; no content asserted.
#[tokio::test]
async fn session_list_returns_array() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/session").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.is_array());
}

// ───────── abort (P1.11) ──────────────────────────────────────────

// [STUB/ROUTE-COVERAGE] abort with no active run; only checks ok:true shape.
#[tokio::test]
async fn session_abort_no_active_run_succeeds() {
    let (_, router) = router_with_state();
    let (_, created) = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let id = created["id"].as_str().unwrap();
    let (s, body) = json_post(
        router.clone(),
        &format!("/session/{id}/abort"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

// [STUB/ROUTE-COVERAGE] interrupt with no active run; only checks 2xx shape.
#[tokio::test]
async fn api_session_interrupt_no_active_run_succeeds() {
    let (_, router) = router_with_state();
    let (_, created) = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let id = created["id"].as_str().unwrap();
    let (s, _body) = json_post(
        router.clone(),
        &format!("/api/session/{id}/interrupt"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

// ───────── messages (P1.12, P1.15) ────────────────────────────────

// [BEHAVIOR] POST message returns a real generated message id.
#[tokio::test]
async fn api_session_message_post_returns_message_id() {
    let (_, router) = router_with_state();
    let (_, created) = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let id = created["id"].as_str().unwrap();
    let (s, body) = json_post(
        router.clone(),
        &format!("/api/session/{id}/message"),
        serde_json::json!({"role": "user"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["id"].is_string());
}

// [BEHAVIOR] create-then-delete message round-trip returns 200.
#[tokio::test]
async fn api_session_message_delete_returns_ok() {
    let (_, router) = router_with_state();
    let (_, created) = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let id = created["id"].as_str().unwrap();
    let (_, body) = json_post(
        router.clone(),
        &format!("/api/session/{id}/message"),
        serde_json::json!({"role": "user"}),
    )
    .await;
    let msg_id = body["id"].as_str().unwrap();
    let s = delete(
        router.clone(),
        &format!("/api/session/{id}/message/{msg_id}"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

// [BEHAVIOR] GET for an unknown message/part returns the documented null.
#[tokio::test]
async fn api_session_message_part_get_returns_null_unknown() {
    let (_, router) = router_with_state();
    let (_, created) = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let id = created["id"].as_str().unwrap();
    let (s, body) = json_get(
        router,
        &format!("/api/session/{id}/message/msg_unknown/part/prt_unknown"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.is_null());
}

// ───────── global control (P2.23) ─────────────────────────────────

// [STUB/ROUTE-COVERAGE] only checks 2xx + ok:true shape here.
#[tokio::test]
async fn global_dispose_returns_ok() {
    let (_, router) = router_with_state();
    let (s, body) = json_post(router, "/global/dispose", serde_json::json!({})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

// [STUB/ROUTE-COVERAGE] only checks 2xx + version-is-string shape.
#[tokio::test]
async fn global_version_returns_object() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/global/version").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["version"].is_string());
}

// [BEHAVIOR] PATCH theme persists + a follow-up GET reads it back.
#[tokio::test]
async fn global_config_alias_round_trip() {
    let (_, router) = router_with_state();
    let (s, _body) = patch(
        router.clone(),
        "/global/config",
        serde_json::json!({"theme": "loom"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, after) = json_get(router, "/global/config").await;
    assert_eq!(after["theme"], "loom");
}

// ───────── permission/question (P2.18) ────────────────────────────

// [STUB/ROUTE-COVERAGE] /api/permission/pending is an empty stub; shape only.
#[tokio::test]
async fn api_permission_pending_returns_data_envelope() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/permission/pending").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// [STUB/ROUTE-COVERAGE] /api/question/pending is an empty stub; shape only.
#[tokio::test]
async fn api_question_pending_returns_data_envelope() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/question/pending").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// ───────── tui control (P1.16) ────────────────────────────────────

// [STUB/ROUTE-COVERAGE] tui control stub; only checks ok:true shape.
#[tokio::test]
async fn control_next_returns_ok() {
    let (_, router) = router_with_state();
    let (s, body) = json_post(router, "/tui/control/next", serde_json::json!({})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

// [STUB/ROUTE-COVERAGE] tui control stub; only checks ok:true shape.
#[tokio::test]
async fn control_exit_returns_ok() {
    let (_, router) = router_with_state();
    let (s, body) = json_post(router, "/tui/control/exit", serde_json::json!({})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

// ───────── instance/mcp/pty/etc (P2.20) ──────────────────────────

// [STUB/ROUTE-COVERAGE] only checks 2xx + kind-is-string shape.
#[tokio::test]
async fn instance_metadata_returns_object() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/instance").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["kind"].is_string());
}

// [STUB/ROUTE-COVERAGE] /api/mcp is a stub; data shape only.
#[tokio::test]
async fn api_mcp_status_returns_object() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/mcp").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// [STUB/ROUTE-COVERAGE] /api/pty is a stub; data shape only.
#[tokio::test]
async fn api_pty_list_returns_envelope() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/pty").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// [STUB/ROUTE-COVERAGE] /find is a stub; matches envelope shape only.
#[tokio::test]
async fn find_results_in_envelope() {
    let (_, router) = router_with_state();
    let (s, body) = json_post(router, "/find", serde_json::json!({"pattern": "tests"})).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("matches").is_some());
}

// ───────── experimental (P2.21) ──────────────────────────────────

// [STUB/ROUTE-COVERAGE] fixed capability flags; shape only.
#[tokio::test]
async fn experimental_capabilities_matches_current_sdk() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/experimental/capabilities").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["backgroundSubagents"], true);
    assert_eq!(body["agents"], true);
}

// ───────── new contract routes (W2 provider / W3 credential / W4 pty) ─
// After the W4 route-table cleanup removed the worktree/revert/mcp-connect/
// oauth/provider-auth groups, this section smoke-tests the NEW contract
// surface that replaced them:
//   GET    /api/provider          (200) — also covered by bootstrap_v2_api_provider_list
//   PATCH  /api/credential/:id    (204) — mutates a seeded credential's label
//   DELETE /api/credential/:id    (204) — idempotent removal
//   GET    /api/pty               (200) — also covered by api_pty_list_returns_envelope

// [STUB/ROUTE-COVERAGE] tiny contract smoke (W2): GET /api/provider -> 200.
#[tokio::test]
async fn smoke_get_api_provider_returns_ok() {
    let (_, router) = router_with_state();
    let (s, _) = json_get(router, "/api/provider").await;
    assert_eq!(s, StatusCode::OK);
}

// [STUB/ROUTE-COVERAGE] tiny contract smoke (W3): PATCH /api/credential/:id -> 2xx.
#[tokio::test]
async fn smoke_patch_api_credential_returns_2xx() {
    let (state, router) = router_with_state();
    let id = "cred_smoke_patch";
    state.credentials.write().insert(
        id.to_string(),
        loom_server::state::CredentialEntry {
            label: "before".to_string(),
            value: None,
        },
    );
    let (s, _) = patch(
        router,
        &format!("/api/credential/{id}"),
        serde_json::json!({"label": "after"}),
    )
    .await;
    assert!(s.is_success(), "PATCH /api/credential/:id must be 2xx, got {s}");
}

// [STUB/ROUTE-COVERAGE] tiny contract smoke (W4): GET /api/pty -> 200.
#[tokio::test]
async fn smoke_get_api_pty_returns_ok() {
    let (_, router) = router_with_state();
    let (s, _) = json_get(router, "/api/pty").await;
    assert_eq!(s, StatusCode::OK);
}

// [BEHAVIOR] PATCH persists a label change on a seeded credential → 204.
#[tokio::test]
async fn api_credential_patch_updates_label_and_returns_no_content() {
    let (state, router) = router_with_state();
    let id = "cred_smoke";
    state.credentials.write().insert(
        id.to_string(),
        loom_server::state::CredentialEntry {
            label: "before".to_string(),
            value: None,
        },
    );
    let (s, _body) = patch(
        router,
        &format!("/api/credential/{id}"),
        serde_json::json!({"label": "after"}),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(
        state.credentials.read().get(id).unwrap().label,
        "after"
    );
}

// [BEHAVIOR] DELETE removes the entry and is idempotent → 204 each time.
#[tokio::test]
async fn api_credential_delete_is_idempotent_and_returns_no_content() {
    let (state, router) = router_with_state();
    let id = "cred_smoke";
    state.credentials.write().insert(
        id.to_string(),
        loom_server::state::CredentialEntry {
            label: "tmp".to_string(),
            value: None,
        },
    );
    // First delete removes the entry.
    assert_eq!(
        delete(router.clone(), &format!("/api/credential/{id}")).await,
        StatusCode::NO_CONTENT
    );
    assert!(state.credentials.read().get(id).is_none());
    // Second delete is idempotent.
    assert_eq!(
        delete(router, &format!("/api/credential/{id}")).await,
        StatusCode::NO_CONTENT
    );
}

// ───────── rollout completion coverage ────────────────────────────

// [BEHAVIOR] multi-endpoint contract asserting specific keys/types exist.
#[tokio::test]
async fn v1_bootstrap_shapes_match_the_current_sdk_contract() {
    let (_, router) = router_with_state();

    let (_, providers) = json_get(router.clone(), "/config/providers").await;
    assert!(providers["providers"].is_array());
    assert!(providers["default"].is_object());

    let (_, path) = json_get(router.clone(), "/path").await;
    for key in ["home", "state", "config", "worktree", "directory"] {
        assert!(path[key].is_string(), "missing path.{key}");
    }

    let (_, project) = json_get(router.clone(), "/project/current").await;
    assert!(project["id"].is_string());
    assert!(project["time"]["created"].is_number());

    let (_, commands) = json_get(router, "/command").await;
    assert!(commands.is_array());
}

// [BEHAVIOR] verifies a credential header is accepted and NOT leaked back.
#[tokio::test]
async fn authorization_headers_are_accepted_without_leaking_into_responses() {
    let (_, router) = router_with_state();
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/global/health")
                .header(header::AUTHORIZATION, "Basic dXNlcjpzZWNyZXQ=")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), MAX_BODY).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("secret"));
}

// [BEHAVIOR] full message + part create/patch/get/delete CRUD round-trip.
#[tokio::test]
async fn v1_message_and_part_crud_uses_info_parts_shape() {
    let (_, router) = router_with_state();
    let (_, session) = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let session_id = session["id"].as_str().unwrap();
    let (_, message) = json_post(
        router.clone(),
        &format!("/api/session/{session_id}/message"),
        serde_json::json!({"role": "assistant"}),
    )
    .await;
    let message_id = message["id"].as_str().unwrap();

    let (status, messages) =
        json_get(router.clone(), &format!("/session/{session_id}/message")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(messages[0]["info"]["id"].is_string());
    assert!(messages[0]["parts"].is_array());

    let (status, part) = patch(
        router.clone(),
        &format!("/session/{session_id}/message/{message_id}/part/prt_test"),
        serde_json::json!({"type": "text", "text": "hello"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(part["messageID"], message_id);

    let (status, message) = json_get(
        router.clone(),
        &format!("/session/{session_id}/message/{message_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(message["parts"][0]["text"], "hello");

    assert_eq!(
        delete(
            router,
            &format!("/session/{session_id}/message/{message_id}/part/prt_test")
        )
        .await,
        StatusCode::OK
    );
}

// [BEHAVIOR] SSE content-type + session-scoped replay filtering (LS-002).
#[tokio::test]
async fn session_event_endpoint_is_sse_and_replays_only_that_session() {
    let (state, router) = router_with_state();
    loom_server::state::emit(
        &state,
        "message.updated",
        serde_json::json!({"sessionID": "sess_replay", "n": 1}),
    );
    let cursor = state
        .event_buffer
        .read()
        .back()
        .unwrap()
        .payload
        .event_id
        .clone();
    loom_server::state::emit(
        &state,
        "message.updated",
        serde_json::json!({"sessionID": "sess_replay", "n": 2}),
    );
    loom_server::state::emit(
        &state,
        "message.updated",
        serde_json::json!({"sessionID": "sess_other", "n": 3}),
    );

    let response = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/api/session/sess_replay/event?after={cursor}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("text/event-stream"),
        "{content_type}"
    );

    let mut body = response.into_body().into_data_stream();
    let mut wire = String::new();
    for _ in 0..3 {
        let chunk = tokio::time::timeout(std::time::Duration::from_secs(1), body.next())
            .await
            .expect("sse chunk")
            .expect("sse body")
            .expect("sse bytes");
        wire.push_str(&String::from_utf8_lossy(&chunk));
        if wire.contains("\"n\":2") {
            break;
        }
    }
    assert!(wire.contains("server.connected"), "{wire}");
    assert!(wire.contains("\"sessionID\":\"sess_replay\""), "{wire}");
    assert!(wire.contains("\"n\":2"), "{wire}");
    assert!(!wire.contains("\"n\":1"), "{wire}");
    assert!(!wire.contains("\"n\":3"), "{wire}");
}

// [BEHAVIOR] real shell execution + output persisted to message parts.
#[tokio::test]
async fn shell_endpoint_executes_and_persists_output() {
    let (_, router) = router_with_state();
    let (_, session) = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let session_id = session["id"].as_str().unwrap();
    let (status, response) = json_post(
        router.clone(),
        &format!("/session/{session_id}/shell"),
        serde_json::json!({"command": "echo loom-shell"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(response["parts"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("loom-shell"));

    let (_, messages) = json_get(router, &format!("/session/{session_id}/message")).await;
    assert_eq!(messages.as_array().unwrap().len(), 1);
    assert_eq!(messages[0]["info"]["mode"], "shell");
}

// [BEHAVIOR] fork/share create children and copy parts into them.
#[tokio::test]
async fn share_and_fork_return_sessions_and_copy_parts() {
    let (_, router) = router_with_state();
    let (_, session) = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let session_id = session["id"].as_str().unwrap();
    let (_, message) = json_post(
        router.clone(),
        &format!("/api/session/{session_id}/message"),
        serde_json::json!({"role": "assistant"}),
    )
    .await;
    let message_id = message["id"].as_str().unwrap();
    let _ = patch(
        router.clone(),
        &format!("/session/{session_id}/message/{message_id}/part/prt_fork"),
        serde_json::json!({"type": "text", "text": "copy me"}),
    )
    .await;

    let (_, shared) = json_post(
        router.clone(),
        &format!("/session/{session_id}/share"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(shared["id"], session_id);
    assert!(shared["share"]["url"].is_string());

    let (_, forked) = json_post(
        router.clone(),
        &format!("/session/{session_id}/fork"),
        serde_json::json!({}),
    )
    .await;
    let fork_id = forked["id"].as_str().unwrap();
    assert_eq!(forked["parentID"], session_id);
    let (_, messages) = json_get(router, &format!("/session/{fork_id}/message")).await;
    assert_eq!(messages[0]["parts"][0]["text"], "copy me");
    assert_eq!(messages[0]["info"]["sessionID"], fork_id);
}

// [BEHAVIOR] dispose clears all sessions from state.
#[tokio::test]
async fn global_dispose_cancels_and_clears_state() {
    let (_, router) = router_with_state();
    let _ = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let (status, _) = json_post(router.clone(), "/global/dispose", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let (_, sessions) = json_get(router, "/session").await;
    assert_eq!(sessions.as_array().unwrap().len(), 0);
}

// === BEHAVIOR TESTS ===
// LS-004 — minimum UI-visible event sequence for a single prompt.
//
// Uses the test-only fake runner hook (`agent_runner::emit_minimal_prompt_sequence`)
// to broadcast the canonical ordered sequence WITHOUT a real LLM call, then asserts
// the event buffer preserves insertion order, matching the opencode contract the TUI
// relies on for one prompt turn:
//   1. message.updated        (user message)
//   2. message.updated        (assistant message)
//   3. message.part.updated   (assistant text part)
//   4. message.part.delta     (streaming delta)
//   5. message.updated        (assistant final)
//   6. session.status         (busy — run started)
//   7. session.status         (idle — run finished)
//
// This is a deterministic contract test: it does not invoke an LLM. It locks the
// *ordering* guarantee (LS-004) the session-scoped SSE stream (LS-002) must
// preserve. The real prompt run path emits these same events through the
// translator; here the fake runner hook emits them directly so the test is fast
// and hermetic.
//
// The sequence is emitted against a freshly created session. Session creation
// itself seeds a leading `session.created` event, so we assert the seven emitted
// events form the ordered *tail* of the buffer rather than the whole buffer.
//
// This test does NOT call a real LLM — it drives the test-only fake runner hook
// — but it verifies a real ordering contract (the exact tail sequence + busy→idle
// lifecycle + sessionID propagation), so it is a behavior test, not a stub.
// [BEHAVIOR] LS-004: asserts the ordered visible event sequence for one prompt turn.
#[tokio::test]
async fn prompt_event_sequence_minimal() {
    let (state, router) = router_with_state();

    // (a) create a session so we have a real sessionID to filter on.
    let (_, session) = json_post(router, "/session", serde_json::json!({})).await;
    let session_id = session["id"].as_str().unwrap().to_string();

    // (b) emit the minimum visible prompt sequence via the test-only fake runner
    // hook (no real LLM call). The hook emits the seven events in the documented
    // order directly onto the SSE bus.
    loom_server::agent_runner::emit_minimal_prompt_sequence(&state, &session_id);

    // (c) collect events from the buffer (snapshot_replay preserves insertion order).
    let events = loom_server::state::snapshot_replay(&state, None);

    // (d) assert the emitted events appear in the correct order. They must be
    // the ordered tail of the buffer (session creation seeded a leading event).
    let expected_types = [
        "message.updated",
        "message.updated",
        "message.part.updated",
        "message.part.delta",
        "message.updated",
        "session.status",
        "session.status",
    ];
    let tail_len = expected_types.len();
    let actual_types: Vec<&str> = events
        .iter()
        .map(|ev| ev.payload.event_type.as_str())
        .collect();
    let actual_tail: Vec<&str> = actual_types
        .iter()
        .rev()
        .take(tail_len)
        .rev()
        .copied()
        .collect();
    assert_eq!(
        actual_tail, expected_types,
        "minimum prompt event sequence must be ordered: user message, assistant \
         message, part updated, part delta, assistant final, busy, idle"
    );

    // Every event in the sequence must carry the originating sessionID so the
    // session-scoped SSE stream (LS-002) can filter it correctly.
    let sequence_events = events.iter().rev().take(tail_len).rev().collect::<Vec<_>>();
    for ev in sequence_events {
        assert_eq!(
            ev.payload.properties["sessionID"].as_str(),
            Some(session_id.as_str()),
            "event '{}' must carry sessionID",
            ev.payload.event_type
        );
    }

    // The lifecycle must go busy -> idle (never the reverse).
    let statuses: Vec<&str> = events
        .iter()
        .filter(|ev| ev.payload.event_type == "session.status")
        .map(|ev| {
            ev.payload.properties["status"]["type"]
                .as_str()
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(statuses, ["busy", "idle"], "busy must precede idle");
}
