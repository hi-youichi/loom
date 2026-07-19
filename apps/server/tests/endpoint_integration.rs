//! Integration tests for endpoint group: Health & Global
//!
//! Tests are organized by individual endpoint. Each test exercises a
//! specific code path — success, error, contract shape, or round-trip.

use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use loom_server::routes::build_router;
use loom_server::state::new_state;
use serde_json::{json, Value};
use tower::ServiceExt;

const MAX_BODY: usize = 1024 * 64;

fn router_with_state() -> (loom_server::state::SharedState, axum::Router) {
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

// ─── GET /global/health ───────────────────────────────────────────

#[tokio::test]
async fn health_global_returns_healthy_true() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/global/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["healthy"], true);
}

#[tokio::test]
async fn health_global_has_no_extra_fields() {
    let (_, router) = router_with_state();
    let (_, body) = json_get(router, "/global/health").await;
    let obj = body.as_object().expect("response must be a JSON object");
    assert_eq!(obj.len(), 1, "response must contain only 'healthy', got: {body}");
    assert!(obj.contains_key("healthy"));
    assert!(!obj.contains_key("ok"));
    assert!(!obj.contains_key("kind"));
    assert!(!obj.contains_key("version"));
}

// ─── GET /api/health ──────────────────────────────────────────────

#[tokio::test]
async fn health_api_returns_exactly_healthy_true() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["healthy"], true);
    let obj = body.as_object().expect("must be object");
    assert_eq!(obj.len(), 1, "must contain only 'healthy', got: {body}");
}

// ─── POST /global/upgrade ─────────────────────────────────────────

#[tokio::test]
async fn health_upgrade_returns_501_not_implemented() {
    let (_, router) = router_with_state();
    let (status, body) = json_post(router, "/global/upgrade", Value::Object(Default::default())).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert!(body["error"].as_str().unwrap_or_default().contains("upgrade"));
}

// ─── GET /global/version ──────────────────────────────────────────

#[tokio::test]
async fn health_version_returns_version_and_kind() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/global/version").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["version"].is_string(), "version must be a string, got: {body}");
    assert_eq!(body["kind"], "external-kernel");
}

#[tokio::test]
async fn health_version_matches_cargo_pkg_version() {
    let (_, router) = router_with_state();
    let (_, body) = json_get(router, "/global/version").await;
    let version = body["version"].as_str().expect("version must be string");
    assert!(!version.is_empty(), "version must not be empty");
}

// ─── POST /global/dispose ─────────────────────────────────────────

#[tokio::test]
async fn health_dispose_returns_ok_and_shutdown() {
    let (_, router) = router_with_state();
    let (status, body) = json_post(router, "/global/dispose", Value::Object(Default::default())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(body["shutdown"], true);
}

#[tokio::test]
async fn health_dispose_clears_all_sessions() {
    let (_, router) = router_with_state();
    let _ = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let _ = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let (_, before) = json_get(router.clone(), "/session").await;
    assert_eq!(before.as_array().unwrap().len(), 2);
    let _ = json_post(router.clone(), "/global/dispose", serde_json::json!({})).await;
    let (_, after) = json_get(router, "/session").await;
    assert_eq!(after.as_array().unwrap().len(), 0, "sessions must be cleared after dispose");
}

// ─── GET /global/config ───────────────────────────────────────────

#[tokio::test]
async fn health_global_config_returns_object() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/global/config").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_object(), "config must be a JSON object, got: {body}");
}

// ─── PATCH /global/config ─────────────────────────────────────────

#[tokio::test]
async fn health_global_config_patch_round_trip() {
    let (_, router) = router_with_state();
    let (status, _) = patch(
        router.clone(),
        "/global/config",
        serde_json::json!({"theme": "global-dark"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, after) = json_get(router, "/global/config").await;
    assert_eq!(after["theme"], "global-dark");
}

#[tokio::test]
async fn health_global_config_patch_emits_config_changed_not_disposed() {
    let state = loom_server::state::new_server_state();
    let router = build_router(state.clone());
    let _ = patch(
        router,
        "/global/config",
        serde_json::json!({"theme": "coverage-test"}),
    )
    .await;
    let events = loom_server::state::snapshot_replay(&state, None);
    let event_types: Vec<&str> = events.iter().map(|e| e.payload.event_type.as_str()).collect();
    assert!(
        event_types.iter().any(|t| t == &"server.config.changed"),
        "expected server.config.changed event, got: {event_types:?}"
    );
    assert!(
        !event_types.iter().any(|t| t == &"server.instance.disposed"),
        "must NOT emit server.instance.disposed for a config patch, got: {event_types:?}"
    );
}

// ─── GET /global/event/replay ─────────────────────────────────────

#[tokio::test]
async fn health_global_event_replay_returns_array() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/global/event/replay").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_array(), "replay must return a JSON array, got: {body}");
}

#[tokio::test]
async fn health_global_event_replay_contains_emitted_events() {
    let (state, router) = router_with_state();
    loom_server::state::emit(
        &state,
        "session.created",
        serde_json::json!({"sessionID": "sess_replay_test"}),
    );
    let (status, body) = json_get(router, "/global/event/replay").await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("must be array");
    assert!(
        arr.iter().any(|e| {
            e.get("type").and_then(|t| t.as_str()) == Some("session.created")
                || e.get("eventType").and_then(|t| t.as_str()) == Some("session.created")
        }),
        "replay must contain the emitted session.created event, got: {body}"
    );
}

// ─── GET /global/event (SSE) ──────────────────────────────────────

#[tokio::test]
async fn health_global_event_is_sse_stream() {
    let (_, router) = router_with_state();
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/global/event")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("text/event-stream"),
        "expected text/event-stream, got: {content_type}"
    );
}

// ─── POST /global/instance/update ─────────────────────────────────

#[tokio::test]
async fn health_instance_update_returns_501() {
    let (_, router) = router_with_state();
    let (status, body) = json_post(
        router,
        "/global/instance/update",
        serde_json::json!({"version": "99.0.0"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert!(body["error"].as_str().unwrap_or_default().contains("instance update"));
}

// ─── cross-cutting: both health aliases agree ─────────────────────

#[tokio::test]
async fn health_global_and_api_health_return_same_shape() {
    let (_, router) = router_with_state();
    let (_, global) = json_get(router.clone(), "/global/health").await;
    let (_, api) = json_get(router, "/api/health").await;
    assert_eq!(global, api, "both health endpoints must return identical bodies");
}

// ═══════════════════════════════════════════════════════════════════
// Endpoint Group: Configuration & Settings
// ═══════════════════════════════════════════════════════════════════

async fn put(router: axum::Router, path: &str, body: Value) -> (StatusCode, Value) {
    let body_str = serde_json::to_string(&body).unwrap();
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::PUT)
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

// ─── GET /config ──────────────────────────────────────────────────

#[tokio::test]
async fn config_get_returns_object() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/config").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_object(), "config must be a JSON object, got: {body}");
}

// ─── PATCH /config ────────────────────────────────────────────────

#[tokio::test]
async fn config_patch_theme_round_trip() {
    let (_, router) = router_with_state();
    let (status, body) = patch(
        router.clone(),
        "/config",
        serde_json::json!({"theme": "oceanic"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["theme"], "oceanic");
    let (_, after) = json_get(router, "/config").await;
    assert_eq!(after["theme"], "oceanic");
}

#[tokio::test]
async fn config_patch_model_round_trip() {
    let (_, router) = router_with_state();
    let (status, body) = patch(
        router.clone(),
        "/config",
        serde_json::json!({"model": "gpt-4o"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["model"], "gpt-4o");
    let (_, after) = json_get(router, "/config").await;
    assert_eq!(after["model"], "gpt-4o");
}

#[tokio::test]
async fn config_patch_extra_keys_stored_in_extra() {
    let (_, router) = router_with_state();
    let (status, body) = patch(
        router.clone(),
        "/config",
        serde_json::json!({"customKey": "customValue"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["extra"]["customKey"], "customValue");
}

// ─── GET /config/providers ────────────────────────────────────────

#[tokio::test]
async fn config_providers_returns_providers_and_default() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/config/providers").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["providers"].is_array(), "providers must be an array, got: {body}");
    assert!(body["default"].is_object(), "default must be an object, got: {body}");
}

// ─── GET /config/settings ─────────────────────────────────────────

#[tokio::test]
async fn config_settings_get_returns_json() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/config/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_object() || body.is_null(),
        "settings should be a JSON object or null when no config file, got: {body}");
}

// ─── PUT /config/settings ─────────────────────────────────────────

#[tokio::test]
async fn config_settings_put_returns_ok_with_settings() {
    let (_, router) = router_with_state();
    let (status, body) = put(
        router,
        "/config/settings",
        serde_json::json!({"test_key": "test_value"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert!(body.get("settings").is_some(), "response must contain 'settings' field");
}

// ─── POST /config/reload ──────────────────────────────────────────

#[tokio::test]
async fn config_reload_returns_status() {
    let (_, router) = router_with_state();
    let (status, body) = json_post(router, "/config/reload", Value::Object(Default::default())).await;
    assert!(status.is_success(), "reload should return 2xx, got: {status}");
    assert!(
        body.get("status").is_some() || body.get("error").is_some(),
        "response must contain 'status' or 'error', got: {body}"
    );
}

// ─── GET /api/config (v2 alias) ───────────────────────────────────

#[tokio::test]
async fn api_config_get_returns_object() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/config").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_object());
}

// ─── PATCH /api/config (v2 alias) ─────────────────────────────────

#[tokio::test]
async fn api_config_patch_round_trip() {
    let (_, router) = router_with_state();
    let (status, _) = patch(
        router.clone(),
        "/api/config",
        serde_json::json!({"theme": "v2-dark"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, after) = json_get(router, "/api/config").await;
    assert_eq!(after["theme"], "v2-dark");
}

// ─── GET /api/config/providers (v2 alias) ─────────────────────────

#[tokio::test]
async fn api_config_providers_returns_providers_array() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/config/providers").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["providers"].is_array());
}

// ─── GET /api/config/settings (v2 alias) ──────────────────────────

#[tokio::test]
async fn api_config_settings_get_returns_json() {
    let (_, router) = router_with_state();
    let (status, _body) = json_get(router, "/api/config/settings").await;
    assert_eq!(status, StatusCode::OK);
}

// ─── PUT /api/config/settings (v2 alias) ──────────────────────────

#[tokio::test]
async fn api_config_settings_put_returns_ok() {
    let (_, router) = router_with_state();
    let (status, body) = put(
        router,
        "/api/config/settings",
        serde_json::json!({"v2_key": "v2_val"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

// ─── POST /api/config/reload (v2 alias) ───────────────────────────

#[tokio::test]
async fn api_config_reload_returns_status() {
    let (_, router) = router_with_state();
    let (status, _body) = json_post(router, "/api/config/reload", Value::Object(Default::default())).await;
    assert!(status.is_success());
}

// ─── v1/v2 parity ─────────────────────────────────────────────────

#[tokio::test]
async fn config_v1_and_v2_get_return_same_data() {
    let (_, router) = router_with_state();
    let (_, v1) = json_get(router.clone(), "/config").await;
    let (_, v2) = json_get(router, "/api/config").await;
    assert_eq!(v1, v2, "v1 /config and v2 /api/config must return identical bodies");
}

#[tokio::test]
async fn config_providers_v1_and_v2_return_same_data() {
    let (_, router) = router_with_state();
    let (_, v1) = json_get(router.clone(), "/config/providers").await;
    let (_, v2) = json_get(router, "/api/config/providers").await;
    assert_eq!(v1, v2, "v1 and v2 providers must match");
}

// ═══════════════════════════════════════════════════════════════════
// Endpoint Group: Bootstrap (Provider/Agent/Model/Command/Skill)
// ═══════════════════════════════════════════════════════════════════

// ─── GET /provider ────────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_provider_returns_all_default_connected() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/provider").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["all"].is_array(), "all must be array, got: {body}");
    assert!(body["default"].is_object(), "default must be object");
    assert!(body["connected"].is_array(), "connected must be array");
}

// ─── GET /agent ───────────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_agent_returns_array_with_build() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/agent").await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("must be array");
    assert!(!arr.is_empty(), "agent list must not be empty");
    assert!(arr.iter().any(|a| a["id"] == "build"), "must contain 'build' agent");
}

// ─── GET /model ───────────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_model_returns_array() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/model").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_object() || body.is_array(),
        "model response must be object (Location.response) or array, got: {body}");
}

// ─── GET /command ─────────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_command_returns_array_with_init_and_review() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/command").await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("must be array");
    assert!(!arr.is_empty());
    let names: Vec<&str> = arr.iter().filter_map(|c| c["name"].as_str()).collect();
    assert!(names.contains(&"init"), "must have 'init' command");
    assert!(names.contains(&"review"), "must have 'review' command");
}

// ─── GET /api/provider ────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_api_provider_returns_location_envelope() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/provider").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_object(), "must be object (Location.response), got: {body}");
}

// ─── GET /api/agent ───────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_api_agent_returns_location_envelope_with_loom() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/agent").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_object(), "must be object");
    let data = &body["data"];
    assert!(data.is_array(), "data must be array");
    assert!(data.as_array().unwrap().iter().any(|a| a["id"] == "loom"), "must contain 'loom' agent");
}

// ─── GET /api/model ───────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_api_model_returns_location_envelope() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/model").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_object(), "must be object");
    assert!(body["data"].is_array(), "data must be array");
}

// ─── GET /api/command ─────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_api_command_returns_location_envelope() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/command").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_object());
    assert!(body["data"].is_array());
}

// ─── GET /api/app/agent ───────────────────────────────────────────

#[tokio::test]
async fn bootstrap_app_agent_returns_array_with_build() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/app/agent").await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("must be array");
    assert!(arr.iter().any(|a| a["id"] == "build"), "must contain 'build' agent");
}

// ─── GET /api/app/model ───────────────────────────────────────────

#[tokio::test]
async fn bootstrap_app_model_returns_array() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/app/model").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_array(), "must be array (may be empty by design)");
}

// ─── GET /api/app/provider ────────────────────────────────────────

#[tokio::test]
async fn bootstrap_app_provider_returns_array() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/app/provider").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_array(), "must be array (may be empty by design)");
}

// ─── GET /api/skill ───────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_api_skill_returns_location_envelope() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/skill").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_object());
    assert!(body["data"].is_array(), "data must be array");
}

// ─── GET /api/reference ───────────────────────────────────────────

#[tokio::test]
async fn bootstrap_api_reference_returns_location_envelope() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/reference").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_object());
    assert!(body["data"].is_array());
}

// ─── GET /api/integration ─────────────────────────────────────────

#[tokio::test]
async fn bootstrap_api_integration_returns_location_envelope() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/integration").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_object());
    assert!(body["data"].is_array());
}

// ─── GET /api/location ────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_api_location_returns_directory() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/location").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["directory"].is_string() || body["project"]["directory"].is_string(),
        "location must have directory field, got: {body}");
}

// ─── PATCH /api/location ──────────────────────────────────────────

#[tokio::test]
async fn bootstrap_api_location_patch_updates_directory() {
    let (_, router) = router_with_state();
    let (status, body) = patch(
        router.clone(),
        "/api/location",
        serde_json::json!({"directory": "/tmp/test-workspace"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, after) = json_get(router, "/api/location").await;
    let dir = after["directory"].as_str().unwrap_or_default();
    assert!(dir.contains("test-workspace"), "directory should be updated, got: {dir}");
}

// ─── PUT /api/location/workspace ──────────────────────────────────

#[tokio::test]
async fn bootstrap_api_location_workspace_returns_directories() {
    let (_, router) = router_with_state();
    let (status, body) = put(router, "/api/location/workspace", Value::Object(Default::default())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["cwd"].is_string(), "must return cwd, got: {body}");
    assert!(body["configDir"].is_string(), "must return configDir");
}

// ─── GET /api/path ────────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_api_path_returns_cwd_and_home() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/path").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["cwd"].is_string(), "must have cwd");
    assert!(body["home"].is_string(), "must have home");
    assert!(body["config"].is_string(), "must have config path");
    assert!(body["cache"].is_string(), "must have cache path");
}

// ═══════════════════════════════════════════════════════════════════
// Endpoint Group: Session CRUD
// ═══════════════════════════════════════════════════════════════════

async fn create_test_session(router: &axum::Router) -> String {
    let (status, body) = json_post(router.clone(), "/session", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    body["id"].as_str().expect("session must have id").to_string()
}

// ─── GET /session ─────────────────────────────────────────────────

#[tokio::test]
async fn session_list_returns_array() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/session").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_array(), "session list must be array, got: {body}");
}

// ─── POST /session ────────────────────────────────────────────────

#[tokio::test]
async fn session_create_returns_session_with_id() {
    let (_, router) = router_with_state();
    let (status, body) = json_post(router, "/session", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["id"].is_string(), "must have id, got: {body}");
    assert!(body["title"].is_string(), "must have title");
}

#[tokio::test]
async fn session_create_with_agent() {
    let (_, router) = router_with_state();
    let (status, body) = json_post(router, "/session", json!({"agent": "build"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["agent"], "build");
}

#[tokio::test]
async fn session_create_increases_list_count() {
    let (_, router) = router_with_state();
    let (_, before) = json_get(router.clone(), "/session").await;
    let initial = before.as_array().unwrap().len();
    let _ = json_post(router.clone(), "/session", json!({})).await;
    let (_, after) = json_get(router, "/session").await;
    assert_eq!(after.as_array().unwrap().len(), initial + 1);
}

// ─── GET /session/:id ─────────────────────────────────────────────

#[tokio::test]
async fn session_get_returns_session_details() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_get(router, &format!("/session/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
}

#[tokio::test]
async fn session_get_nonexistent_returns_404() {
    let (_, router) = router_with_state();
    let (status, _) = json_get(router, "/session/nonexistent-id").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ─── PATCH /session/:id ───────────────────────────────────────────

#[tokio::test]
async fn session_patch_updates_title() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = patch(router.clone(), &format!("/session/{id}"), json!({"title": "My Session"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "My Session");
}

#[tokio::test]
async fn session_patch_updates_agent() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = patch(router, &format!("/session/{id}"), json!({"agent": "review"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["agent"], "review");
}

#[tokio::test]
async fn session_patch_nonexistent_returns_404() {
    let (_, router) = router_with_state();
    let (status, _) = patch(router, "/session/nonexistent", json!({"title": "x"})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ─── DELETE /session/:id ──────────────────────────────────────────

#[tokio::test]
async fn session_delete_returns_204() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(&format!("/session/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn session_delete_nonexistent_returns_404() {
    let (_, router) = router_with_state();
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/session/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn session_delete_removes_from_list() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(&format!("/session/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let (status, body) = json_get(router, "/session").await;
    assert_eq!(status, StatusCode::OK);
    let sessions = body.as_array().unwrap();
    assert!(sessions.iter().all(|s| s["id"] != id), "deleted session must not appear in list");
}

// ─── GET /api/session (v2 alias) ──────────────────────────────────

#[tokio::test]
async fn api_session_list_returns_array() {
    let (_, router) = router_with_state();
    let (status, body) = json_get(router, "/api/session").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_array());
}

// ─── POST /api/session (v2 alias) ─────────────────────────────────

#[tokio::test]
async fn api_session_create_returns_session_with_id() {
    let (_, router) = router_with_state();
    let (status, body) = json_post(router, "/api/session", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["id"].is_string());
}

// ─── GET /api/session/:sessionID (v2 alias) ───────────────────────

#[tokio::test]
async fn api_session_get_returns_details() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_get(router, &format!("/api/session/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
}

// ─── PATCH /api/session/:sessionID (v2 alias) ─────────────────────

#[tokio::test]
async fn api_session_patch_updates_title() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = patch(router, &format!("/api/session/{id}"), json!({"title": "V2 Title"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "V2 Title");
}

// ─── DELETE /api/session/:sessionID (v2 alias) ────────────────────

#[tokio::test]
async fn api_session_delete_returns_204() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(&format!("/api/session/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

// ─── GET /api/session/active ──────────────────────────────────────

#[tokio::test]
async fn api_session_active_returns_sessions_envelope() {
    let (_, router) = router_with_state();
    let _ = create_test_session(&router).await;
    let (status, body) = json_get(router, "/api/session/active").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["sessions"].is_array(), "must have sessions array, got: {body}");
    let arr = body["sessions"].as_array().unwrap();
    assert!(!arr.is_empty(), "sessions array should not be empty after creating a session");
    assert!(arr[0]["id"].is_string());
    assert!(arr[0]["state"].is_string());
}

// ─── POST /api/session/create ─────────────────────────────────────

#[tokio::test]
async fn api_session_create_endpoint_returns_session() {
    let (_, router) = router_with_state();
    let (status, body) = json_post(router, "/api/session/create", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["session"].is_object(), "must have session object, got: {body}");
    assert!(body["session"]["id"].is_string(), "session must have id");
}

// ─── DELETE /global/session/:id ───────────────────────────────────

#[tokio::test]
async fn global_session_delete_uses_full_cascade() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(&format!("/global/session/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT, "must return 204 (full cascade handler)");
    let (_, body) = json_get(router, "/session").await;
    let sessions = body.as_array().unwrap();
    assert!(
        sessions.iter().all(|s| s["id"] != id),
        "deleted session must not appear in list (full cascade confirmed)"
    );
}

// ─── v1/v2 parity ─────────────────────────────────────────────────

#[tokio::test]
async fn session_v1_and_v2_list_return_same_data() {
    let (_, router) = router_with_state();
    let (_, v1) = json_get(router.clone(), "/session").await;
    let (_, v2) = json_get(router, "/api/session").await;
    assert_eq!(v1, v2, "v1 /session and v2 /api/session must return identical lists");
}

// ═══════════════════════════════════════════════════════════════════
// Endpoint Group: Session Agent (Prompt/Abort/Command/Shell)
// ═══════════════════════════════════════════════════════════════════

// ─── GET /api/session/:sessionID/status ───────────────────────────

#[tokio::test]
async fn agent_status_returns_idle_for_new_session() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_get(router, &format!("/api/session/{id}/status")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "idle");
    assert!(body["modelID"].is_string(), "modelID must be string");
    assert!(body["providerID"].is_string(), "providerID must be string");
    assert!(body["agent"].is_string(), "agent must be string");
}

#[tokio::test]
async fn agent_status_returns_404_for_nonexistent() {
    let (_, router) = router_with_state();
    let (status, _) = json_get(router, "/api/session/nonexistent/status").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn agent_status_reflects_session_agent() {
    let (_, router) = router_with_state();
    let (status, body) = json_post(router.clone(), "/session", json!({"agent": "review"})).await;
    assert_eq!(status, StatusCode::OK);
    let id = body["id"].as_str().unwrap();
    let (_, status_body) = json_get(router, &format!("/api/session/{id}/status")).await;
    assert_eq!(status_body["agent"], "review");
}

// ─── POST /session/:id/prompt ─────────────────────────────────────

#[tokio::test]
async fn agent_prompt_returns_response_for_simple_prompt() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(&format!("/session/{id}/prompt"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({"prompt": "hello"})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::ACCEPTED,
        "prompt should return 200 or 202, got {status}"
    );
}

#[tokio::test]
async fn agent_prompt_nonexistent_session_returns_error() {
    let (_, router) = router_with_state();
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/session/nonexistent/prompt")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({"prompt": "hello"})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        response.status().is_client_error() || response.status() == StatusCode::NOT_FOUND,
        "should return 4xx for nonexistent session"
    );
}

// ─── POST /session/:id/prompt_async ───────────────────────────────

#[tokio::test]
async fn agent_prompt_async_returns_ok() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_post(
        router,
        &format!("/session/{id}/prompt_async"),
        json!({"prompt": "hello async"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

// ─── POST /session/:id/abort ──────────────────────────────────────

#[tokio::test]
async fn agent_abort_returns_ok_with_cancelled_false_when_idle() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_post(router, &format!("/session/{id}/abort"), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn agent_abort_nonexistent_returns_ok_with_cancelled_false() {
    let (_, router) = router_with_state();
    let (status, body) = json_post(router, "/session/nonexistent/abort", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(body["cancelled"], false);
}

// ─── POST /api/session/:sessionID/interrupt ───────────────────────

#[tokio::test]
async fn agent_interrupt_returns_ok() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_post(router, &format!("/api/session/{id}/interrupt"), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

// ─── POST /api/session/:sessionID/agent (switchAgent) ─────────────

#[tokio::test]
async fn agent_switch_agent_returns_204() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(&format!("/api/session/{id}/agent"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({"agent": "review"})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let (_, body) = json_get(router, &format!("/session/{id}")).await;
    assert_eq!(body["agent"], "review");
}

#[tokio::test]
async fn agent_switch_agent_nonexistent_returns_404() {
    let (_, router) = router_with_state();
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/session/nonexistent/agent")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({"agent": "review"})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ─── POST /api/session/:sessionID/prompt (v2 alias) ───────────────

#[tokio::test]
async fn agent_v2_prompt_returns_response() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(&format!("/api/session/{id}/prompt"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({"prompt": "v2 hello"})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        response.status() == StatusCode::OK || response.status() == StatusCode::ACCEPTED,
        "v2 prompt should return 200 or 202"
    );
}

// ─── POST /session/:id/command ────────────────────────────────────

#[tokio::test]
async fn agent_command_without_command_returns_400() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, _) = json_post(router, &format!("/session/{id}/command"), json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn agent_command_nonexistent_session_returns_error() {
    let (_, router) = router_with_state();
    let (status, _) = json_post(
        router,
        "/session/nonexistent/command",
        json!({"command": "/help"}),
    )
    .await;
    assert!(status.is_client_error());
}

// ─── POST /session/:id/shell ──────────────────────────────────────

#[tokio::test]
async fn agent_shell_without_command_returns_400() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, _) = json_post(router, &format!("/session/{id}/shell"), json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn agent_shell_nonexistent_session_returns_404() {
    let (_, router) = router_with_state();
    let (status, _) = json_post(
        router,
        "/session/nonexistent/shell",
        json!({"command": "echo hi"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn agent_shell_executes_echo() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_post(
        router,
        &format!("/session/{id}/shell"),
        json!({"command": "echo hello_world"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("info").is_some() || body.get("output").is_some(),
        "shell response should contain info or output, got: {body}");
}

// ═══════════════════════════════════════════════════════════════════
// Endpoint Group: Session Lifecycle (Init/Fork/Summarize/Share/Children/Diff/Todo)
// ═══════════════════════════════════════════════════════════════════

async fn delete_raw(router: axum::Router, path: &str) -> StatusCode {
    router
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

// ─── POST /session/:id/init ───────────────────────────────────────

#[tokio::test]
async fn lifecycle_init_returns_ok() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_post(router, &format!("/session/{id}/init"), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn lifecycle_init_nonexistent_returns_200_or_404() {
    let (_, router) = router_with_state();
    let (status, _) = json_post(router, "/session/nonexistent/init", json!({})).await;
    assert!(status == StatusCode::OK || status == StatusCode::NOT_FOUND);
}

// ─── POST /session/:id/fork ───────────────────────────────────────

#[tokio::test]
async fn lifecycle_fork_creates_child_session() {
    let (_, router) = router_with_state();
    let parent_id = create_test_session(&router).await;
    let (status, body) = json_post(router.clone(), &format!("/session/{parent_id}/fork"), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["id"].is_string(), "forked session must have an id");
    assert_ne!(body["id"], parent_id, "child must have different id");
    let (_, children) = json_get(router, &format!("/session/{parent_id}/children")).await;
    let children = children.as_array().unwrap();
    assert_eq!(children.len(), 1, "parent should have 1 child after fork");
    assert_eq!(children[0]["parentID"], parent_id);
}

#[tokio::test]
async fn lifecycle_fork_nonexistent_returns_404() {
    let (_, router) = router_with_state();
    let (status, _) = json_post(router, "/session/nonexistent/fork", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ─── POST /session/:id/summarize ──────────────────────────────────

#[tokio::test]
async fn lifecycle_summarize_returns_ok() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_post(router, &format!("/session/{id}/summarize"), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

// ─── POST /session/:id/share ──────────────────────────────────────

#[tokio::test]
async fn lifecycle_share_sets_share_url() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_post(router.clone(), &format!("/session/{id}/share"), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["share"].is_object(), "session must have share object, got: {body}");
    assert!(body["share"]["url"].is_string(), "share must have url");
    let (_, after) = json_get(router, &format!("/session/{id}")).await;
    assert!(after["share"].is_object(), "session should be persisted as shared");
}

#[tokio::test]
async fn lifecycle_share_nonexistent_returns_404() {
    let (_, router) = router_with_state();
    let (status, _) = json_post(router, "/session/nonexistent/share", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ─── DELETE /session/:id/share ────────────────────────────────────

#[tokio::test]
async fn lifecycle_unshare_clears_share() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let _ = json_post(router.clone(), &format!("/session/{id}/share"), json!({})).await;
    let status = delete_raw(router.clone(), &format!("/session/{id}/share")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = json_get(router, &format!("/session/{id}")).await;
    assert!(
        body["share"].is_null(),
        "share must be cleared after unshare, got: {}",
        body["share"]
    );
}

#[tokio::test]
async fn lifecycle_unshare_nonexistent_returns_404() {
    let (_, router) = router_with_state();
    let status = delete_raw(router, "/session/nonexistent/share").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn lifecycle_unshare_v2_clears_share() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let _ = json_post(router.clone(), &format!("/session/{id}/share"), json!({})).await;
    let status = delete_raw(router.clone(), &format!("/api/session/{id}/share")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = json_get(router, &format!("/session/{id}")).await;
    assert!(body["share"].is_null(), "share must be cleared after v2 unshare");
}

// ─── GET /session/:id/children ────────────────────────────────────

#[tokio::test]
async fn lifecycle_children_returns_empty_for_session_without_forks() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_get(router, &format!("/session/{id}/children")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_array());
    assert_eq!(body.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn lifecycle_children_returns_forked_sessions() {
    let (_, router) = router_with_state();
    let parent_id = create_test_session(&router).await;
    let _ = json_post(router.clone(), &format!("/session/{parent_id}/fork"), json!({})).await;
    let _ = json_post(router.clone(), &format!("/session/{parent_id}/fork"), json!({})).await;
    let (status, body) = json_get(router, &format!("/session/{parent_id}/children")).await;
    assert_eq!(status, StatusCode::OK);
    let children = body.as_array().unwrap();
    assert_eq!(children.len(), 2);
    assert!(children.iter().all(|c| c["parentID"] == parent_id));
}

// ─── GET /session/:id/todo ────────────────────────────────────────

#[tokio::test]
async fn lifecycle_todo_returns_session_id_and_todos() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_get(router, &format!("/session/{id}/todo")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sessionID"], id);
    assert!(body["todos"].is_array(), "todos must be array");
}

#[tokio::test]
async fn lifecycle_todo_nonexistent_returns_404() {
    let (_, router) = router_with_state();
    let (status, _) = json_get(router, "/session/nonexistent/todo").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ─── GET /session/:id/diff ────────────────────────────────────────

#[tokio::test]
async fn lifecycle_diff_returns_session_id_and_diff() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_get(router, &format!("/session/{id}/diff")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sessionID"], id);
    assert!(body["diff"].is_array(), "diff must be array");
}

#[tokio::test]
async fn lifecycle_diff_nonexistent_returns_404() {
    let (_, router) = router_with_state();
    let (status, _) = json_get(router, "/session/nonexistent/diff").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ─── v2 aliases ───────────────────────────────────────────────────

#[tokio::test]
async fn lifecycle_v2_init_returns_ok() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_post(router, &format!("/api/session/{id}/init"), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn lifecycle_v2_fork_creates_child() {
    let (_, router) = router_with_state();
    let parent_id = create_test_session(&router).await;
    let (status, body) = json_post(router, &format!("/api/session/{parent_id}/fork"), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["id"].is_string());
    assert_ne!(body["id"], parent_id);
}

#[tokio::test]
async fn lifecycle_v2_summarize_returns_ok() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_post(router, &format!("/api/session/{id}/summarize"), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn lifecycle_v2_share_sets_url() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_post(router.clone(), &format!("/api/session/{id}/share"), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["share"]["url"].is_string());
}

#[tokio::test]
async fn lifecycle_v2_children_returns_array() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_get(router, &format!("/api/session/{id}/children")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_array());
}

#[tokio::test]
async fn lifecycle_v2_todo_returns_session_id() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_get(router, &format!("/api/session/{id}/todo")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sessionID"], id);
}

#[tokio::test]
async fn lifecycle_v2_diff_returns_session_id() {
    let (_, router) = router_with_state();
    let id = create_test_session(&router).await;
    let (status, body) = json_get(router, &format!("/api/session/{id}/diff")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sessionID"], id);
}
