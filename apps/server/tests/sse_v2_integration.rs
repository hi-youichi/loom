//! Wire-level integration tests for Loom's v2 SSE endpoints.
//!
//! These tests intentionally consume the bytes emitted by the Axum SSE response
//! instead of inspecting the v2 event log.  `all_v2_event_shapes_are_emitted`
//! is the structural coverage gate for the 32 OpenCode `session.next.*` types.

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
};
use futures::StreamExt;
use loom_server::{
    routes::build_router,
    state::{new_state, SharedState},
    v2_event::{publish_durable, publish_live},
};
use serde_json::{json, Map, Value};
use tower::ServiceExt;

const SESSION: &str = "sess_sse_integration";

fn router() -> (SharedState, axum::Router) {
    let state = new_state();
    let router = build_router(state.clone());
    (state, router)
}

fn data(ty: &str) -> Value {
    let base = json!({"timestamp": 1, "sessionID": SESSION});
    let base = base.as_object().unwrap().clone();
    match ty {
        "session.next.agent.switched" => {
            json!({"timestamp":1,"sessionID":SESSION,"messageID":"msg_1","agent":"build"})
        }
        "session.next.model.switched" => {
            json!({"timestamp":1,"sessionID":SESSION,"messageID":"msg_1","model":{"id":"m","providerID":"p","variant":"fast"}})
        }
        "session.next.moved" => {
            json!({"timestamp":1,"sessionID":SESSION,"location":{"directory":"C:/work","workspaceID":"ws_1"},"subdirectory":"src"})
        }
        "session.next.prompted" | "session.next.prompt.admitted" => {
            json!({"timestamp":1,"sessionID":SESSION,"messageID":"msg_1","prompt":{"text":"hi"},"delivery":"queue"})
        }
        "session.next.context.updated" | "session.next.synthetic" => {
            json!({"timestamp":1,"sessionID":SESSION,"messageID":"msg_1","text":"note"})
        }
        "session.next.shell.started" => {
            json!({"timestamp":1,"sessionID":SESSION,"messageID":"msg_1","callID":"call_1","command":"echo ok"})
        }
        "session.next.shell.ended" => {
            json!({"timestamp":1,"sessionID":SESSION,"callID":"call_1","output":"ok"})
        }
        "session.next.step.started" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","agent":"build","model":{"id":"m","providerID":"p","variant":"fast"},"snapshot":"snap_1"})
        }
        "session.next.step.ended" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","finish":"stop","cost":0.0,"tokens":{"input":1.0,"output":1.0,"reasoning":0.0,"cache":{"read":0.0,"write":0.0}},"snapshot":"snap_1","files":["src/main.rs"]})
        }
        "session.next.step.failed" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","error":{"type":"unknown","message":"failed"}})
        }
        "session.next.text.started" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","textID":"text_1"})
        }
        "session.next.text.delta" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","textID":"text_1","delta":"x"})
        }
        "session.next.text.ended" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","textID":"text_1","text":"x"})
        }
        "session.next.reasoning.started" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","reasoningID":"reason_1","providerMetadata":{"p":{"key":"v"}}})
        }
        "session.next.reasoning.delta" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","reasoningID":"reason_1","delta":"x"})
        }
        "session.next.reasoning.ended" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","reasoningID":"reason_1","text":"x","providerMetadata":{"p":{"key":"v"}}})
        }
        "session.next.tool.input.started" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","callID":"call_1","name":"read"})
        }
        "session.next.tool.input.delta" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","callID":"call_1","delta":"{"})
        }
        "session.next.tool.input.ended" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","callID":"call_1","text":"{}"})
        }
        "session.next.tool.called" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","callID":"call_1","tool":"read","input":{},"provider":{"executed":false,"metadata":{"p":{"key":"v"}}}})
        }
        "session.next.tool.progress" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","callID":"call_1","structured":{},"content":[{"type":"text","text":"running"}]})
        }
        "session.next.tool.success" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","callID":"call_1","structured":{},"content":[{"type":"file","uri":"file:///x","mime":"text/plain","name":"x"}],"outputPaths":["x"],"result":{"ok":true},"provider":{"executed":false,"metadata":{"p":{"key":"v"}}}})
        }
        "session.next.tool.failed" => {
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","callID":"call_1","error":{"type":"unknown","message":"failed"},"result":{"code":"E"},"provider":{"executed":false,"metadata":{"p":{"key":"v"}}}})
        }
        "session.next.retried" => {
            json!({"timestamp":1,"sessionID":SESSION,"attempt":1.0,"error":{"message":"retry","isRetryable":true,"statusCode":429.0,"responseHeaders":{"x":"y"},"responseBody":"body","metadata":{"source":"mock"}}})
        }
        "session.next.compaction.started" => {
            json!({"timestamp":1,"sessionID":SESSION,"messageID":"msg_1","reason":"manual"})
        }
        "session.next.compaction.delta" => {
            json!({"timestamp":1,"sessionID":SESSION,"messageID":"msg_1","text":"summary"})
        }
        "session.next.compaction.ended" => {
            json!({"timestamp":1,"sessionID":SESSION,"messageID":"msg_1","reason":"manual","text":"summary","recent":"recent"})
        }
        "session.next.revert.staged" => {
            json!({"timestamp":1,"sessionID":SESSION,"revert":{"messageID":"msg_1","partID":"prt_1","snapshot":"snap","diff":"diff","files":[{"path":"a.rs","status":"modified","additions":1,"deletions":0,"patch":"+x"}]}})
        }
        "session.next.revert.cleared" => Value::Object(base),
        "session.next.revert.committed" => {
            json!({"timestamp":1,"sessionID":SESSION,"messageID":"msg_1"})
        }
        _ => panic!("missing fixture for {ty}"),
    }
}

fn definitions() -> &'static [(&'static str, bool)] {
    &[
        ("session.next.agent.switched", true),
        ("session.next.model.switched", true),
        ("session.next.moved", true),
        ("session.next.prompted", true),
        ("session.next.prompt.admitted", true),
        ("session.next.context.updated", true),
        ("session.next.synthetic", true),
        ("session.next.shell.started", true),
        ("session.next.shell.ended", true),
        ("session.next.step.started", true),
        ("session.next.step.ended", true),
        ("session.next.step.failed", true),
        ("session.next.text.started", true),
        ("session.next.text.delta", false),
        ("session.next.text.ended", true),
        ("session.next.reasoning.started", true),
        ("session.next.reasoning.delta", false),
        ("session.next.reasoning.ended", true),
        ("session.next.tool.input.started", true),
        ("session.next.tool.input.delta", false),
        ("session.next.tool.input.ended", true),
        ("session.next.tool.called", true),
        ("session.next.tool.progress", true),
        ("session.next.tool.success", true),
        ("session.next.tool.failed", true),
        ("session.next.retried", true),
        ("session.next.compaction.started", true),
        ("session.next.compaction.delta", false),
        ("session.next.compaction.ended", true),
        ("session.next.revert.staged", true),
        ("session.next.revert.cleared", true),
        ("session.next.revert.committed", true),
    ]
}

fn json_frames(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(bytes)
        .split("\n\n")
        .filter_map(|frame| frame.lines().find_map(|line| line.strip_prefix("data: ")))
        .map(|data| serde_json::from_str(data).expect("SSE data is JSON"))
        .collect()
}

async fn next_json(body: Body) -> Value {
    let mut stream = body.into_data_stream();
    let mut wire = Vec::new();
    loop {
        let chunk = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .expect("SSE frame timeout")
            .expect("SSE stream unexpectedly closed")
            .expect("SSE bytes");
        wire.extend_from_slice(&chunk);
        if let Some(frame) = json_frames(&wire).pop() {
            return frame;
        }
    }
}

fn require_string(object: &Map<String, Value>, name: &str) {
    assert!(
        object.get(name).and_then(Value::as_str).is_some(),
        "missing string {name}: {object:?}"
    );
}
fn require_number(object: &Map<String, Value>, name: &str) {
    assert!(
        object
            .get(name)
            .and_then(Value::as_f64)
            .is_some_and(f64::is_finite),
        "missing finite number {name}: {object:?}"
    );
}
fn require_object<'a>(object: &'a Map<String, Value>, name: &str) -> &'a Map<String, Value> {
    object
        .get(name)
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("missing object {name}: {object:?}"))
}

fn assert_no_null(value: &Value) {
    match value {
        Value::Null => panic!("v2 optional fields must be absent, not null"),
        Value::Array(xs) => xs.iter().for_each(assert_no_null),
        Value::Object(xs) => xs.values().for_each(assert_no_null),
        _ => {}
    }
}

fn assert_data_keys(payload: &Map<String, Value>, ty: &str) {
    let (required, optional): (&[&str], &[&str]) = match ty {
        "session.next.agent.switched" => (&["timestamp", "sessionID", "messageID", "agent"], &[]),
        "session.next.model.switched" => (&["timestamp", "sessionID", "messageID", "model"], &[]),
        "session.next.moved" => (&["timestamp", "sessionID", "location"], &["subdirectory"]),
        "session.next.prompted" | "session.next.prompt.admitted" => (
            &["timestamp", "sessionID", "messageID", "prompt", "delivery"],
            &[],
        ),
        "session.next.context.updated" | "session.next.synthetic" => {
            (&["timestamp", "sessionID", "messageID", "text"], &[])
        }
        "session.next.shell.started" => (
            &["timestamp", "sessionID", "messageID", "callID", "command"],
            &[],
        ),
        "session.next.shell.ended" => (&["timestamp", "sessionID", "callID", "output"], &[]),
        "session.next.step.started" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "agent",
                "model",
            ],
            &["snapshot"],
        ),
        "session.next.step.ended" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "finish",
                "cost",
                "tokens",
            ],
            &["snapshot", "files"],
        ),
        "session.next.step.failed" => (
            &["timestamp", "sessionID", "assistantMessageID", "error"],
            &[],
        ),
        "session.next.text.started" => (
            &["timestamp", "sessionID", "assistantMessageID", "textID"],
            &[],
        ),
        "session.next.text.delta" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "textID",
                "delta",
            ],
            &[],
        ),
        "session.next.text.ended" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "textID",
                "text",
            ],
            &[],
        ),
        "session.next.reasoning.started" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "reasoningID",
            ],
            &["providerMetadata"],
        ),
        "session.next.reasoning.delta" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "reasoningID",
                "delta",
            ],
            &[],
        ),
        "session.next.reasoning.ended" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "reasoningID",
                "text",
            ],
            &["providerMetadata"],
        ),
        "session.next.tool.input.started" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "callID",
                "name",
            ],
            &[],
        ),
        "session.next.tool.input.delta" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "callID",
                "delta",
            ],
            &[],
        ),
        "session.next.tool.input.ended" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "callID",
                "text",
            ],
            &[],
        ),
        "session.next.tool.called" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "callID",
                "tool",
                "input",
                "provider",
            ],
            &[],
        ),
        "session.next.tool.progress" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "callID",
                "structured",
                "content",
            ],
            &[],
        ),
        "session.next.tool.success" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "callID",
                "structured",
                "content",
                "provider",
            ],
            &["outputPaths", "result"],
        ),
        "session.next.tool.failed" => (
            &[
                "timestamp",
                "sessionID",
                "assistantMessageID",
                "callID",
                "error",
                "provider",
            ],
            &["result"],
        ),
        "session.next.retried" => (&["timestamp", "sessionID", "attempt", "error"], &[]),
        "session.next.compaction.started" => {
            (&["timestamp", "sessionID", "messageID", "reason"], &[])
        }
        "session.next.compaction.delta" => (&["timestamp", "sessionID", "messageID", "text"], &[]),
        "session.next.compaction.ended" => (
            &[
                "timestamp",
                "sessionID",
                "messageID",
                "reason",
                "text",
                "recent",
            ],
            &[],
        ),
        "session.next.revert.staged" => (&["timestamp", "sessionID", "revert"], &[]),
        "session.next.revert.cleared" => (&["timestamp", "sessionID"], &[]),
        "session.next.revert.committed" => (&["timestamp", "sessionID", "messageID"], &[]),
        _ => panic!("missing structural registry entry for {ty}"),
    };
    for key in required {
        assert!(payload.contains_key(*key), "{ty} misses required {key}");
    }
    for key in payload.keys() {
        assert!(
            required.contains(&key.as_str()) || optional.contains(&key.as_str()),
            "{ty} has unknown data key {key}"
        );
    }
}

fn assert_event(value: &Value, ty: &str, durable: bool) {
    assert_no_null(value);
    let event = value.as_object().expect("event object");
    for key in event.keys() {
        assert!(
            ["id", "metadata", "type", "durable", "location", "data"].contains(&key.as_str()),
            "unknown outer key {key}"
        );
    }
    require_string(event, "id");
    assert_eq!(event["type"], ty);
    let payload = require_object(event, "data");
    assert_data_keys(payload, ty);
    require_number(payload, "timestamp");
    assert_eq!(
        payload.get("sessionID").and_then(Value::as_str),
        Some(SESSION)
    );
    match (durable, event.get("durable")) {
        (true, Some(Value::Object(d))) => {
            assert_eq!(d["aggregateID"], SESSION);
            require_number(d, "seq");
            assert_eq!(
                d["version"],
                if matches!(ty, "session.next.step.ended" | "session.next.step.failed") {
                    2
                } else {
                    1
                }
            );
        }
        (false, None) => {}
        _ => panic!("wrong durable mode for {ty}: {event:?}"),
    }
    match ty {
        "session.next.step.ended" => {
            require_string(payload, "assistantMessageID");
            require_string(payload, "finish");
            require_number(payload, "cost");
            let tokens = require_object(payload, "tokens");
            for field in ["input", "output", "reasoning"] {
                require_number(tokens, field);
            }
            let cache = require_object(tokens, "cache");
            require_number(cache, "read");
            require_number(cache, "write");
        }
        "session.next.step.failed" | "session.next.tool.failed" => {
            let error = require_object(payload, "error");
            assert_eq!(error["type"], "unknown");
            require_string(error, "message");
        }
        "session.next.tool.progress" | "session.next.tool.success" => {
            assert!(
                require_object(payload, "structured").is_empty()
                    || payload["structured"].is_object()
            );
            let content = payload["content"].as_array().expect("tool content array");
            assert!(!content.is_empty());
            for item in content {
                let item = item.as_object().unwrap();
                match item["type"].as_str() {
                    Some("text") => require_string(item, "text"),
                    Some("file") => {
                        require_string(item, "uri");
                        require_string(item, "mime");
                    }
                    other => panic!("invalid tool content {other:?}"),
                }
            }
        }
        "session.next.revert.staged" => {
            let revert = require_object(payload, "revert");
            require_string(revert, "messageID");
            let file = revert["files"].as_array().unwrap()[0].as_object().unwrap();
            require_string(file, "path");
            assert!(matches!(
                file["status"].as_str(),
                Some("added" | "modified" | "deleted")
            ));
            require_number(file, "additions");
            require_number(file, "deletions");
            require_string(file, "patch");
        }
        _ => {}
    }
}

async fn session_sse(router: axum::Router, after: u64) -> (StatusCode, String, axum::body::Body) {
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/api/session/{SESSION}/event?after={after}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    (status, content_type, response.into_body())
}

async fn post_json(router: axum::Router, path: String, body: Value) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

struct TcpServer {
    base: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for TcpServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

async fn spawn_tcp_server(state: SharedState) -> TcpServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback test listener");
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        axum::serve(listener, build_router(state))
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("test server exits cleanly");
    });
    TcpServer {
        base: format!("http://{address}"),
        shutdown: Some(shutdown_tx),
    }
}

#[tokio::test]
async fn tcp_global_sse_emits_message_frame_for_live_events() {
    let state = new_state();
    let server = spawn_tcp_server(state.clone()).await;
    let response = reqwest::Client::new()
        .get(format!("{}/api/event", server.base))
        .send()
        .await
        .expect("open global SSE over TCP");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert!(response.headers()[reqwest::header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/event-stream"));
    publish_live(
        &state,
        "session.next.text.delta",
        data("session.next.text.delta"),
    );
    let mut stream = response.bytes_stream();
    let chunk = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
        .await
        .expect("TCP SSE frame timeout")
        .expect("TCP SSE closes")
        .expect("TCP SSE bytes");
    let wire = String::from_utf8_lossy(&chunk);
    assert!(wire.contains("event: message"), "{wire}");
    let event = json_frames(&chunk).pop().expect("v2 JSON frame");
    assert_event(&event, "session.next.text.delta", false);
    drop(server);
}

#[tokio::test]
async fn global_v2_sse_emits_durable_flat_envelope() {
    let (state, app) = router();
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/event")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    publish_durable(
        &state,
        "session.next.synthetic",
        data("session.next.synthetic"),
        1,
    )
    .unwrap();
    let event = next_json(response.into_body()).await;
    assert_event(&event, "session.next.synthetic", true);
    assert!(
        event.get("payload").is_none(),
        "v2 must not use legacy wrapper"
    );
}

#[tokio::test]
async fn all_v2_event_shapes_are_emitted_through_sse() {
    let mut covered = Vec::new();
    for (ty, durable) in definitions() {
        let (state, app) = router();
        if *durable {
            publish_durable(
                &state,
                *ty,
                data(ty),
                if matches!(*ty, "session.next.step.ended" | "session.next.step.failed") {
                    2
                } else {
                    1
                },
            )
            .expect("durable fixture publishes");
            let (status, content_type, body) = session_sse(app, 0).await;
            assert_eq!(status, StatusCode::OK);
            assert!(content_type.starts_with("text/event-stream"));
            let frame = next_json(body).await;
            assert_event(&frame, ty, true);
        } else {
            let response = app
                .oneshot(
                    Request::builder()
                        .method(Method::GET)
                        .uri("/api/event")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(response.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/event-stream"));
            publish_live(&state, *ty, data(ty));
            let frame = next_json(response.into_body()).await;
            assert_event(&frame, ty, false);
        }
        covered.push(*ty);
    }
    assert_eq!(covered.len(), 32, "structure registry must stay exhaustive");
}

#[tokio::test]
async fn optional_v2_fields_are_omitted_instead_of_serialized_as_null() {
    let cases = [
        (
            "session.next.model.switched",
            json!({"timestamp":1,"sessionID":SESSION,"messageID":"msg_1","model":{"id":"m","providerID":"p","variant":null}}),
            "model.variant",
        ),
        (
            "session.next.moved",
            json!({"timestamp":1,"sessionID":SESSION,"location":{"directory":"C:/work","workspaceID":null},"subdirectory":null}),
            "subdirectory",
        ),
        (
            "session.next.step.started",
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","agent":"build","model":{"id":"m","providerID":"p"},"snapshot":null}),
            "snapshot",
        ),
        (
            "session.next.step.ended",
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","finish":"stop","cost":0.0,"tokens":{"input":0.0,"output":0.0,"reasoning":0.0,"cache":{"read":0.0,"write":0.0}},"snapshot":null,"files":null}),
            "files",
        ),
        (
            "session.next.reasoning.started",
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","reasoningID":"reason_1","providerMetadata":null}),
            "providerMetadata",
        ),
        (
            "session.next.tool.success",
            json!({"timestamp":1,"sessionID":SESSION,"assistantMessageID":"msg_a","callID":"call_1","structured":{},"content":[],"outputPaths":null,"result":null,"provider":{"executed":false,"metadata":null}}),
            "result",
        ),
    ];
    for (ty, input, absent) in cases {
        let (state, app) = router();
        publish_durable(
            &state,
            ty,
            input,
            if ty == "session.next.step.ended" {
                2
            } else {
                1
            },
        )
        .unwrap();
        let (_, _, body) = session_sse(app, 0).await;
        let event = next_json(body).await;
        let payload = event["data"].as_object().unwrap();
        if absent == "model.variant" {
            assert!(payload["model"].get("variant").is_none());
        } else {
            assert!(
                !payload.contains_key(absent),
                "{ty}.{absent} must be absent: {payload:?}"
            );
        }
        assert_no_null(&event);
    }
}

#[tokio::test]
async fn session_sse_replays_only_target_durable_events_and_validates_cursor() {
    let (state, app) = router();
    publish_durable(
        &state,
        "session.next.synthetic",
        data("session.next.synthetic"),
        1,
    )
    .unwrap();
    publish_live(
        &state,
        "session.next.text.delta",
        data("session.next.text.delta"),
    );
    publish_durable(
        &state,
        "session.next.context.updated",
        json!({"timestamp":2,"sessionID":SESSION,"messageID":"msg_1","text":"two"}),
        1,
    )
    .unwrap();
    publish_durable(
        &state,
        "session.next.synthetic",
        json!({"timestamp":3,"sessionID":"sess_other","messageID":"msg_2","text":"other"}),
        1,
    )
    .unwrap();
    let (status, _, body) = session_sse(app.clone(), 1).await;
    assert_eq!(status, StatusCode::OK);
    let frame = next_json(body).await;
    assert_event(&frame, "session.next.context.updated", true);
    for invalid in ["-1", "abc", "18446744073709551616"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/session/{SESSION}/event?after={invalid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "after={invalid}"
        );
    }
}

#[tokio::test]
async fn reconnect_from_last_sequence_replays_each_durable_event_once() {
    let (state, app) = router();
    for (timestamp, text) in [(1, "one"), (2, "two"), (3, "three")] {
        publish_durable(
            &state,
            "session.next.synthetic",
            json!({"timestamp":timestamp,"sessionID":SESSION,"messageID":"msg_1","text":text}),
            1,
        )
        .unwrap();
    }
    let (_, _, body) = session_sse(app.clone(), 0).await;
    let first = next_json(body).await;
    assert_eq!(first["durable"]["seq"], 1);

    let (_, _, body) = session_sse(app, 1).await;
    let second = next_json(body).await;
    assert_eq!(second["durable"]["seq"], 2);
    assert_eq!(second["data"]["text"], "two");
}

#[tokio::test]
async fn subscribed_session_stream_receives_new_durable_event_once() {
    let (state, app) = router();
    let (_, _, body) = session_sse(app, 0).await;
    publish_durable(
        &state,
        "session.next.synthetic",
        data("session.next.synthetic"),
        1,
    )
    .unwrap();
    let event = next_json(body).await;
    assert_event(&event, "session.next.synthetic", true);
    assert_eq!(event["durable"]["seq"], 1);
}

#[tokio::test]
async fn session_handlers_publish_replayable_v2_events() {
    let (_state, app) = router();
    let (status, session) = post_json(app.clone(), "/session".to_string(), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let session_id = session["id"].as_str().expect("created session id");

    let (status, synthetic) = post_json(
        app.clone(),
        format!("/api/session/{session_id}/synthetic"),
        json!({"text":"fixture"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let message_id = synthetic["data"]["id"]
        .as_str()
        .expect("synthetic message id");
    let (status, _) = post_json(
        app.clone(),
        format!("/api/session/{session_id}/compact"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = post_json(
        app.clone(),
        format!("/api/session/{session_id}/revert/stage"),
        json!({"messageID":message_id}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = post_json(
        app.clone(),
        format!("/api/session/{session_id}/revert/commit"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/session/{session_id}/event?after=0"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    let mut events = Vec::new();
    while events.len() < 6 {
        let chunk = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        events.extend(json_frames(&chunk));
    }
    let types: Vec<_> = events
        .iter()
        .map(|event| event["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        types,
        [
            "session.next.synthetic",
            "session.next.compaction.started",
            "session.next.context.updated",
            "session.next.compaction.ended",
            "session.next.revert.staged",
            "session.next.revert.committed",
        ],
        "only durable handler events may be replayed"
    );
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn mock_llm_runs_through_agent_translator_and_session_sse() {
    use loom_llm::client::MockLlm;
    use std::path::PathBuf;
    use tool_core::active_operation::RunCancellation;

    let (state, app) = router();
    loom_server::agent_runner::run_agent_with_test_client(
        state.clone(),
        SESSION.to_string(),
        "msg_mock_assistant".to_string(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        "mock prompt".to_string(),
        None,
        Some("build".to_string()),
        RunCancellation::new(0),
        Box::new(MockLlm::with_no_tool_calls("mock answer").with_stream_by_char()),
    )
    .await
    .expect("mock run succeeds");

    let expected = loom_server::v2_event::replay_after(&state, SESSION, 0);
    assert!(
        !expected.is_empty(),
        "mock agent must emit at least a durable step boundary"
    );
    let (_, _, body) = session_sse(app, 0).await;
    let mut stream = body.into_data_stream();
    let mut types = Vec::new();
    while types.len() < expected.len() {
        let chunk = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .expect("mock SSE frame timeout")
            .expect("mock SSE unexpectedly closed")
            .expect("mock SSE bytes");
        for event in json_frames(&chunk) {
            assert_event(&event, event["type"].as_str().expect("event type"), true);
            types.push(event["type"].as_str().unwrap().to_string());
        }
    }
    assert_eq!(
        types,
        expected
            .iter()
            .map(|event| event.event_type.clone())
            .collect::<Vec<_>>(),
        "SSE replay must reproduce every durable event emitted by the mock run"
    );
    assert!(types.contains(&"session.next.step.started".to_string()));
}
