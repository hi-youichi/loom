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

// ───────── bootstrap (P0.2, P0.3) ─────────────────────────────────

#[tokio::test]
async fn bootstrap_v1_get_config_returns_object() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/config").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.is_object());
}

#[tokio::test]
async fn bootstrap_v2_get_api_config_returns_object() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/config").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.is_object());
}

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

#[tokio::test]
async fn bootstrap_provider_list_matches_v1_sdk_shape() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/provider").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["all"].is_array());
    assert!(body["default"].is_object());
    assert!(body["connected"].is_array());
}

#[tokio::test]
async fn bootstrap_agent_list_includes_build_agent() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/agent").await;
    assert_eq!(s, StatusCode::OK);
    let agents = body.as_array().cloned().unwrap_or_default();
    assert!(!agents.is_empty(), "build agent must be discoverable");
    assert_eq!(agents[0]["name"], "build");
}

#[tokio::test]
async fn bootstrap_lsp_status_is_array() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/lsp/status").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.is_array());
}

#[tokio::test]
async fn bootstrap_formatter_status_is_array() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/formatter/status").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.is_array());
}

#[tokio::test]
async fn bootstrap_session_status_is_session_map() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/session/status").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.is_object());
}

#[tokio::test]
async fn bootstrap_experimental_capabilities_matches_current_sdk() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/experimental/capabilities").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["backgroundSubagents"], true);
}

#[tokio::test]
async fn bootstrap_v2_api_health_ok() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/health").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn bootstrap_global_health_ok() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/global/health").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn bootstrap_v2_api_location_round_trip() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/location").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["id"].is_string());
}

#[tokio::test]
async fn bootstrap_v2_api_path_cwd() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/path").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["cwd"].is_string());
}

#[tokio::test]
async fn bootstrap_v2_api_agent_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/agent").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

#[tokio::test]
async fn bootstrap_v2_api_command_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/command").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

#[tokio::test]
async fn bootstrap_v2_api_provider_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/provider").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

#[tokio::test]
async fn bootstrap_v2_api_model_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/model").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

#[tokio::test]
async fn bootstrap_v2_api_skill_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/skill").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

#[tokio::test]
async fn bootstrap_v2_api_reference_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/reference").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

#[tokio::test]
async fn bootstrap_v2_api_integration_list() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/integration").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

#[tokio::test]
async fn bootstrap_v2_api_vcs_status() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/vcs/status").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["dirty"].is_boolean());
}

#[tokio::test]
async fn bootstrap_v2_api_permission_saved() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/permission/saved").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// ───────── session (P1.13, P1.14) ─────────────────────────────────

#[tokio::test]
async fn session_create_returns_info_with_id() {
    let (_, router) = router_with_state();
    let (s, body) = json_post(router, "/session", serde_json::json!({})).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["id"].is_string());
}

#[tokio::test]
async fn session_get_unknown_returns_404() {
    let (_, router) = router_with_state();
    let (s, _) = json_get(router, "/session/sess_unknown").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn session_delete_unknown_returns_404() {
    let (_, router) = router_with_state();
    let s = delete(router, "/session/sess_unknown").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

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

#[tokio::test]
async fn session_list_returns_array() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/session").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.is_array());
}

// ───────── abort (P1.11) ──────────────────────────────────────────

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

#[tokio::test]
async fn global_dispose_returns_ok() {
    let (_, router) = router_with_state();
    let (s, body) = json_post(router, "/global/dispose", serde_json::json!({})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn global_version_returns_object() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/global/version").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["version"].is_string());
}

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

#[tokio::test]
async fn api_permission_pending_returns_data_envelope() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/permission/pending").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

#[tokio::test]
async fn api_question_pending_returns_data_envelope() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/question/pending").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// ───────── tui control (P1.16) ────────────────────────────────────

#[tokio::test]
async fn control_next_returns_ok() {
    let (_, router) = router_with_state();
    let (s, body) = json_post(router, "/tui/control/next", serde_json::json!({})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn control_exit_returns_ok() {
    let (_, router) = router_with_state();
    let (s, body) = json_post(router, "/tui/control/exit", serde_json::json!({})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

// ───────── instance/mcp/pty/etc (P2.20) ──────────────────────────

#[tokio::test]
async fn instance_metadata_returns_object() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/instance").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["kind"].is_string());
}

#[tokio::test]
async fn api_mcp_status_returns_object() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/mcp").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

#[tokio::test]
async fn api_pty_list_returns_envelope() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/api/pty").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

#[tokio::test]
async fn find_results_in_envelope() {
    let (_, router) = router_with_state();
    let (s, body) = json_post(router, "/find", serde_json::json!({"pattern": "tests"})).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("matches").is_some());
}

// ───────── experimental (P2.21) ──────────────────────────────────

#[tokio::test]
async fn experimental_capabilities_matches_current_sdk() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/experimental/capabilities").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["backgroundSubagents"], true);
    assert_eq!(body["agents"], true);
}

// ───────── provider OAuth (P2.22) ─────────────────────────────────

#[tokio::test]
async fn provider_auth_post_returns_ok() {
    let (_, router) = router_with_state();
    let (s, body) = json_post(
        router,
        "/provider/auth",
        serde_json::json!({"providerID": "openai"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn provider_auth_get_one_returns_status() {
    let (_, router) = router_with_state();
    let (s, body) = json_get(router, "/provider/auth/openai").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.get("data").is_some());
}

// ───────── rollout completion coverage ────────────────────────────

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

#[tokio::test]
async fn session_event_replay_honors_after_cursor() {
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

    let (status, replay) = json_get(
        router,
        &format!("/api/session/sess_replay/event?after={cursor}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["data"].as_array().unwrap().len(), 1);
    assert_eq!(replay["data"][0]["payload"]["properties"]["n"], 2);
    assert_eq!(replay["hasMore"], false);
}

#[tokio::test]
async fn p2_stub_routes_are_registered_with_current_methods() {
    let (_, router) = router_with_state();
    let checks = [
        (Method::GET, "/permission", None),
        (Method::GET, "/question", None),
        (Method::POST, "/question/req/reject", None),
        (Method::PATCH, "/mcp", Some(serde_json::json!({}))),
        (Method::POST, "/mcp/demo/connect", None),
        (Method::PATCH, "/pty/demo", Some(serde_json::json!({}))),
        (Method::GET, "/file/content?path=README.md", None),
        (Method::GET, "/file/status", None),
        (Method::GET, "/find?pattern=src", None),
        (Method::GET, "/experimental/resource/demo", None),
    ];

    for (method, path, body) in checks {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                body.map_or_else(String::new, |value| value.to_string()),
            ))
            .unwrap();
        let response = router.clone().oneshot(request).await.expect("request");
        assert_ne!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_ne!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{path}");
    }
}

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

#[tokio::test]
async fn global_dispose_cancels_and_clears_state() {
    let (_, router) = router_with_state();
    let _ = json_post(router.clone(), "/session", serde_json::json!({})).await;
    let (status, _) = json_post(router.clone(), "/global/dispose", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let (_, sessions) = json_get(router, "/session").await;
    assert_eq!(sessions.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn provider_oauth_mvp_is_explicitly_not_implemented() {
    let (_, router) = router_with_state();
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/provider/demo/oauth/authorize")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
}
