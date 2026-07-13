//! Question request handling (task P2.18).

use axum::{extract::Path, Json};
use serde_json::{json, Value};

/// `POST /question/:requestID/reply` — user replies to a question.
pub async fn post_question_reply(
    Path(request_id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    Json(json!({
        "ok": true,
        "requestID": request_id,
        "answers": body.get("answers").cloned().unwrap_or(json!([])),
    }))
}

/// `POST /api/question/:requestID/reply` — v2 alias.
pub async fn post_api_question_reply(
    Path(request_id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    post_question_reply(Path(request_id), Json(body)).await
}

/// `GET /api/question/pending` — list pending questions.
pub async fn get_api_question_pending() -> Json<Value> {
    Json(json!({ "data": [] }))
}

/// `POST /api/question` — create a question (loom-server can raise it).
pub async fn post_api_question(Json(_body): Json<Value>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// `POST /question` — v1 raising entry.
pub async fn post_question(Json(_body): Json<Value>) -> Json<Value> {
    Json(json!({ "ok": true }))
}

/// `GET /question/pending` — v1 listing.
pub async fn get_question_pending() -> Json<Value> {
    Json(json!([]))
}

pub async fn post_question_reject(Path(_request_id): Path<String>) -> Json<Value> {
    Json(json!(true))
}
