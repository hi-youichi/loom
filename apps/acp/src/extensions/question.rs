use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::oneshot;

use crate::connection::ConnectionOutbound;
use crate::connection_registry::ConnectionRegistry;

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 86_400_000;
const MAX_COMPLETED_IDS: usize = 4096;

fn internal_error(context: &str, error: impl std::fmt::Display) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(format!("{context}: {error}"))),
    }
}

fn object_params(method: &str, params: Value) -> Result<Value, ExtensionError> {
    if params.is_object() {
        Ok(params)
    } else {
        Err(ExtensionError::invalid_params(format!(
            "{method} params must be a JSON object"
        )))
    }
}

fn nonblank(value: &str, field: &str) -> Result<(), ExtensionError> {
    if value.trim().is_empty() {
        Err(ExtensionError::invalid_params(format!(
            "{field} must not be empty"
        )))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionChoice {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionRequest {
    pub question_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<QuestionChoice>,
    #[serde(default)]
    pub allow_free_text: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free_text_placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_choice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionStatus {
    Answered,
    Cancelled,
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionReply {
    pub question_id: String,
    pub status: QuestionStatus,
    pub choice: Option<String>,
    pub free_text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionReplyRequest {
    pub question_id: String,
    pub status: QuestionReplyStatus,
    #[serde(default)]
    pub choice: Option<String>,
    #[serde(default)]
    pub free_text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionReplyStatus {
    Answered,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionReplyResponse {
    pub accepted: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionCancelRequest {
    pub question_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionCancelResponse {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingQuestionState {
    Pending,
    Answered,
    Cancelled,
    TimedOut,
}

struct PendingQuestion {
    request: QuestionRequest,
    owner_connection_id: String,
    owner_session_id: Option<String>,
    state: PendingQuestionState,
    sender: oneshot::Sender<QuestionReply>,
}

#[derive(Default)]
struct StoreState {
    pending: HashMap<String, PendingQuestion>,
    completed: HashSet<String>,
    completed_order: VecDeque<String>,
}

#[derive(Default)]
pub struct QuestionStore {
    state: Mutex<StoreState>,
}

impl QuestionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.state
            .lock()
            .expect("question store mutex poisoned")
            .pending
            .len()
    }

    fn rebind_session(
        &self,
        session_id: &str,
        connection_id: &str,
    ) -> Result<Vec<QuestionRequest>, ExtensionError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal_error("question store lock failed", "mutex poisoned"))?;
        let mut requests = Vec::new();
        for record in state.pending.values_mut() {
            if record.owner_session_id.as_deref() == Some(session_id) {
                record.owner_connection_id = connection_id.to_string();
                requests.push(record.request.clone());
            }
        }
        Ok(requests)
    }

    fn remember_completed(state: &mut StoreState, id: String) {
        if state.completed.insert(id.clone()) {
            state.completed_order.push_back(id);
        }
        while state.completed_order.len() > MAX_COMPLETED_IDS {
            if let Some(old) = state.completed_order.pop_front() {
                state.completed.remove(&old);
            }
        }
    }

    fn insert(
        &self,
        request: QuestionRequest,
        owner_connection_id: String,
        owner_session_id: Option<String>,
        sender: oneshot::Sender<QuestionReply>,
    ) -> Result<(), ExtensionError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal_error("question store lock failed", "mutex poisoned"))?;
        if state.pending.contains_key(&request.question_id)
            || state.completed.contains(&request.question_id)
        {
            return Err(ExtensionError::invalid_params(
                "questionId is already in use",
            ));
        }
        state.pending.insert(
            request.question_id.clone(),
            PendingQuestion {
                request,
                owner_connection_id,
                owner_session_id,
                state: PendingQuestionState::Pending,
                sender,
            },
        );
        Ok(())
    }

    fn take_validated<F>(
        &self,
        id: &str,
        ctx: &ExtensionContext,
        validate: F,
    ) -> Result<Option<PendingQuestion>, ExtensionError>
    where
        F: FnOnce(&PendingQuestion) -> Result<(), ExtensionError>,
    {
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal_error("question store lock failed", "mutex poisoned"))?;
        let Some(record) = state.pending.get(id) else {
            return Ok(None);
        };
        if record.owner_connection_id != ctx.connection_id
            || record.owner_session_id.as_deref() != ctx.session_id.as_deref()
        {
            return Err(ExtensionError::invalid_params(
                "questionId does not belong to this connection or session",
            ));
        }
        validate(record)?;
        let record = state.pending.remove(id).ok_or_else(|| {
            internal_error("question state changed", "pending question disappeared")
        })?;
        Self::remember_completed(&mut state, id.to_string());
        Ok(Some(record))
    }

    fn drop_pending(
        &self,
        id: &str,
        state_value: PendingQuestionState,
    ) -> Result<Option<PendingQuestion>, ExtensionError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal_error("question store lock failed", "mutex poisoned"))?;
        let mut record = state.pending.remove(id);
        if let Some(ref mut value) = record {
            value.state = state_value;
        }
        if record.is_some() {
            Self::remember_completed(&mut state, id.to_string());
        }
        Ok(record)
    }

    fn rollback(&self, id: &str) -> Result<(), ExtensionError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal_error("question store lock failed", "mutex poisoned"))?;
        state.pending.remove(id);
        Ok(())
    }

    fn list_for_connection(
        &self,
        connection_id: &str,
        session_id: Option<&str>,
    ) -> Result<Vec<QuestionRequest>, ExtensionError> {
        let state = self
            .state
            .lock()
            .map_err(|_| internal_error("question store lock failed", "mutex poisoned"))?;
        let mut requests: Vec<QuestionRequest> = state
            .pending
            .values()
            .filter(|record| {
                record.owner_connection_id == connection_id
                    && session_id.is_none_or(|id| record.owner_session_id.as_deref() == Some(id))
            })
            .map(|record| record.request.clone())
            .collect();
        requests.sort_by(|left, right| left.question_id.cmp(&right.question_id));
        Ok(requests)
    }

    pub fn cancel_connection(
        &self,
        connection_id: &str,
        session_id: Option<&str>,
    ) -> Result<usize, ExtensionError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| internal_error("question store lock failed", "mutex poisoned"))?;
        let ids: Vec<String> = state
            .pending
            .iter()
            .filter(|(_, record)| {
                record.owner_connection_id == connection_id
                    && session_id.is_none_or(|id| record.owner_session_id.as_deref() == Some(id))
            })
            .map(|(id, _)| id.clone())
            .collect();
        let mut cancelled = 0;
        for id in ids {
            if let Some(mut record) = state.pending.remove(&id) {
                record.state = PendingQuestionState::Cancelled;
                let _ = record.sender.send(QuestionReply {
                    question_id: id.clone(),
                    status: QuestionStatus::Cancelled,
                    choice: None,
                    free_text: None,
                });
                Self::remember_completed(&mut state, id);
                cancelled += 1;
            }
        }
        Ok(cancelled)
    }
}

pub trait QuestionTransport: Send + Sync {
    fn send_request(&self, request: &QuestionRequest, ctx: &ExtensionContext)
        -> Result<(), String>;
}

pub struct QuestionHandler {
    store: Arc<QuestionStore>,
    transport: Option<Arc<dyn QuestionTransport>>,
    connections: Option<Arc<ConnectionRegistry>>,
}

impl QuestionHandler {
    pub fn new() -> Self {
        Self {
            store: Arc::new(QuestionStore::new()),
            transport: None,
            connections: None,
        }
    }

    pub fn with_store(store: Arc<QuestionStore>) -> Self {
        Self {
            store,
            transport: None,
            connections: None,
        }
    }

    pub fn with_connections(connections: Arc<ConnectionRegistry>) -> Self {
        Self {
            store: Arc::new(QuestionStore::new()),
            transport: None,
            connections: Some(connections),
        }
    }

    pub fn with_transport(
        store: Arc<QuestionStore>,
        transport: Arc<dyn QuestionTransport>,
    ) -> Self {
        Self {
            store,
            transport: Some(transport),
            connections: None,
        }
    }

    pub fn store(&self) -> &Arc<QuestionStore> {
        &self.store
    }

    pub fn cancel_connection(
        &self,
        connection_id: &str,
        session_id: Option<&str>,
    ) -> Result<usize, ExtensionError> {
        self.store.cancel_connection(connection_id, session_id)
    }

    /// Move pending, deadline-bound questions to a replacement transport and
    /// resend their cards. The original timeout task remains authoritative.
    pub async fn rebind_session(
        &self,
        session_id: &str,
        connection_id: &str,
    ) -> Result<usize, ExtensionError> {
        let requests = self.store.rebind_session(session_id, connection_id)?;
        if requests.is_empty() {
            return Ok(0);
        }
        let connections = self.connections.as_ref().ok_or_else(|| {
            internal_error("question rebind failed", "connection registry unavailable")
        })?;
        let connection = connections
            .get(connection_id)
            .ok_or_else(|| internal_error("question rebind failed", "connection not found"))?;
        let capabilities = connection
            .require_capabilities()
            .await
            .map_err(|error| internal_error("question rebind failed", error))?;
        if !capabilities.supports_question() {
            return Err(ExtensionError::capability_not_supported("question"));
        }
        for request in &requests {
            connection
                .outbound_tx
                .send(ConnectionOutbound::ExtensionNotification {
                    method: "_anureo.dev/question/request".into(),
                    params: serde_json::to_value(request)
                        .map_err(|error| internal_error("question rebind failed", error))?,
                })
                .await
                .map_err(|error| internal_error("question rebind failed", error))?;
        }
        Ok(requests.len())
    }

    /// Request a question from an in-process agent tool.
    ///
    /// Agent tools use the same store, ownership checks, timeout handling, and
    /// outbound notification path as the public extension method. Keeping one
    /// request path prevents tool-triggered questions from becoming an
    /// untracked second implementation.
    pub async fn request_for_agent(
        &self,
        request: QuestionRequest,
        connection_id: String,
        session_id: Option<String>,
        client_capabilities: crate::client_capabilities::ClientCapabilitiesInfo,
    ) -> Result<QuestionReply, ExtensionError> {
        let params = serde_json::to_value(request)
            .map_err(|e| internal_error("question request serialization failed", e))?;
        let ctx = ExtensionContext {
            session_id,
            principal: "anureo-agent".into(),
            connection_id,
            working_directory: None,
            client_capabilities,
        };
        let value = ExtensionHandler::handle(self, "request", params, &ctx).await?;
        serde_json::from_value(value)
            .map_err(|e| internal_error("question response deserialization failed", e))
    }
}

impl Default for QuestionHandler {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_request(request: &QuestionRequest) -> Result<u64, ExtensionError> {
    nonblank(&request.question_id, "questionId")?;
    nonblank(&request.prompt, "prompt")?;
    if let Some(title) = &request.title {
        nonblank(title, "title")?;
    }
    let mut values = HashSet::new();
    for choice in &request.choices {
        nonblank(&choice.value, "choices.value")?;
        nonblank(&choice.label, "choices.label")?;
        if !values.insert(&choice.value) {
            return Err(ExtensionError::invalid_params(
                "choice values must be unique",
            ));
        }
        if let Some(description) = &choice.description {
            nonblank(description, "choices.description")?;
        }
    }
    if request.choices.is_empty() && !request.allow_free_text {
        return Err(ExtensionError::invalid_params(
            "a question without choices must allow free text",
        ));
    }
    if request.free_text_placeholder.is_some() && !request.allow_free_text {
        return Err(ExtensionError::invalid_params(
            "freeTextPlaceholder requires free text",
        ));
    }
    if let Some(placeholder) = &request.free_text_placeholder {
        nonblank(placeholder, "freeTextPlaceholder")?;
    }
    if let Some(default) = &request.default_choice {
        nonblank(default, "defaultChoice")?;
        let choice = request
            .choices
            .iter()
            .find(|choice| choice.value == *default)
            .ok_or_else(|| ExtensionError::invalid_params("defaultChoice must match a choice"))?;
        if choice.disabled {
            return Err(ExtensionError::invalid_params(
                "defaultChoice cannot be disabled",
            ));
        }
    }
    let timeout = request.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
    if !(1..=MAX_TIMEOUT_MS).contains(&timeout) {
        return Err(ExtensionError::invalid_params(format!(
            "timeoutMs must be between 1 and {MAX_TIMEOUT_MS}"
        )));
    }
    Ok(timeout)
}

fn validate_reply(
    record: &PendingQuestion,
    reply: &QuestionReplyRequest,
) -> Result<(), ExtensionError> {
    match &reply.status {
        QuestionReplyStatus::Cancelled => {
            if reply.choice.is_some() || reply.free_text.is_some() {
                return Err(ExtensionError::invalid_params(
                    "cancelled replies cannot contain choice or freeText",
                ));
            }
        }
        QuestionReplyStatus::Answered => {
            if !record.request.choices.is_empty() {
                let choice = reply
                    .choice
                    .as_deref()
                    .ok_or_else(|| ExtensionError::invalid_params("choice is required"))?;
                let allowed = record
                    .request
                    .choices
                    .iter()
                    .any(|item| item.value == choice && !item.disabled);
                if !allowed {
                    return Err(ExtensionError::invalid_params("choice is not allowed"));
                }
            } else if reply.choice.is_some() {
                return Err(ExtensionError::invalid_params(
                    "choice is not valid without choices",
                ));
            }
            if let Some(free_text) = &reply.free_text {
                if !record.request.allow_free_text {
                    return Err(ExtensionError::invalid_params("freeText is not allowed"));
                }
                nonblank(free_text, "freeText")?;
            }
            if record.request.choices.is_empty() && reply.free_text.is_none() {
                return Err(ExtensionError::invalid_params("freeText is required"));
            }
        }
    }
    Ok(())
}

struct PendingGuard {
    store: Arc<QuestionStore>,
    id: String,
    active: bool,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        if self.active {
            if let Ok(Some(record)) = self
                .store
                .drop_pending(&self.id, PendingQuestionState::Cancelled)
            {
                let _ = record.sender.send(QuestionReply {
                    question_id: self.id.clone(),
                    status: QuestionStatus::Cancelled,
                    choice: None,
                    free_text: None,
                });
            }
        }
    }
}

#[async_trait]
impl ExtensionHandler for QuestionHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "list" => {
                let params = object_params(method, params)?;
                let session_id = params.get("sessionId").and_then(Value::as_str);
                let requests = self
                    .store
                    .list_for_connection(&ctx.connection_id, session_id)?;
                serde_json::to_value(requests)
                    .map_err(|e| internal_error("question list serialization failed", e))
            }
            "request" => {
                if !ctx.client_capabilities.supports_question() {
                    return Err(ExtensionError::capability_not_supported("question"));
                }
                let value = object_params("request", params)?;
                let request: QuestionRequest = serde_json::from_value(value).map_err(|e| {
                    ExtensionError::invalid_params(format!("invalid request params: {e}"))
                })?;
                let timeout_ms = validate_request(&request)?;
                if request.session_id.is_some()
                    && request.session_id.as_deref() != ctx.session_id.as_deref()
                {
                    return Err(ExtensionError::invalid_params(
                        "sessionId must match the current session",
                    ));
                }
                let id = request.question_id.clone();
                let owner_session_id = ctx.session_id.clone();
                let (sender, receiver) = oneshot::channel();
                self.store.insert(
                    request.clone(),
                    ctx.connection_id.clone(),
                    owner_session_id,
                    sender,
                )?;
                let mut guard = PendingGuard {
                    store: self.store.clone(),
                    id: id.clone(),
                    active: true,
                };
                if let Some(transport) = &self.transport {
                    if let Err(error) = transport.send_request(&request, ctx) {
                        guard.active = false;
                        self.store.rollback(&id)?;
                        return Err(internal_error("question request transport failed", error));
                    }
                }
                if let Some(connections) = &self.connections {
                    let connection = connections.get(&ctx.connection_id).ok_or_else(|| {
                        internal_error("question request transport failed", "connection not found")
                    })?;
                    let params = serde_json::to_value(&request).map_err(|error| {
                        internal_error("question request serialization failed", error)
                    })?;
                    connection
                        .outbound_tx
                        .send(ConnectionOutbound::ExtensionNotification {
                            method: "_anureo.dev/question/request".into(),
                            params,
                        })
                        .await
                        .map_err(|error| {
                            internal_error("question request transport failed", error)
                        })?;
                }
                let result =
                    match tokio::time::timeout(Duration::from_millis(timeout_ms), receiver).await {
                        Ok(Ok(reply)) => reply,
                        Ok(Err(_)) => {
                            return Err(internal_error(
                                "question reply channel failed",
                                "channel closed",
                            ))
                        }
                        Err(_) => {
                            if let Ok(Some(record)) =
                                self.store.drop_pending(&id, PendingQuestionState::TimedOut)
                            {
                                let _ = record.sender.send(QuestionReply {
                                    question_id: id.clone(),
                                    status: QuestionStatus::Timeout,
                                    choice: record.request.default_choice.clone(),
                                    free_text: None,
                                });
                            }
                            QuestionReply {
                                question_id: id.clone(),
                                status: QuestionStatus::Timeout,
                                choice: request.default_choice.clone(),
                                free_text: None,
                            }
                        }
                    };
                guard.active = false;
                serde_json::to_value(result)
                    .map_err(|e| internal_error("question response serialization failed", e))
            }
            "reply" => {
                let value = object_params("reply", params)?;
                let reply: QuestionReplyRequest = serde_json::from_value(value).map_err(|e| {
                    ExtensionError::invalid_params(format!("invalid reply params: {e}"))
                })?;
                nonblank(&reply.question_id, "questionId")?;
                let Some(record) =
                    self.store
                        .take_validated(&reply.question_id, ctx, |record| {
                            validate_reply(record, &reply)
                        })?
                else {
                    let state = self.store.state.lock().map_err(|_| {
                        internal_error("question store lock failed", "mutex poisoned")
                    })?;
                    if state.completed.contains(&reply.question_id) {
                        return serde_json::to_value(QuestionReplyResponse { accepted: false })
                            .map_err(|e| internal_error("reply serialization failed", e));
                    }
                    return Err(ExtensionError::invalid_params("unknown questionId"));
                };
                let status = match reply.status {
                    QuestionReplyStatus::Answered => QuestionStatus::Answered,
                    QuestionReplyStatus::Cancelled => QuestionStatus::Cancelled,
                };
                let result = QuestionReply {
                    question_id: reply.question_id.clone(),
                    status,
                    choice: reply.choice,
                    free_text: reply.free_text,
                };
                record.sender.send(result).map_err(|_| {
                    internal_error("question reply delivery failed", "channel closed")
                })?;
                serde_json::to_value(QuestionReplyResponse { accepted: true })
                    .map_err(|e| internal_error("reply serialization failed", e))
            }
            "cancel" => {
                let value = object_params("cancel", params)?;
                let cancel: QuestionCancelRequest = serde_json::from_value(value).map_err(|e| {
                    ExtensionError::invalid_params(format!("invalid cancel params: {e}"))
                })?;
                nonblank(&cancel.question_id, "questionId")?;
                let Some(record) =
                    self.store
                        .take_validated(&cancel.question_id, ctx, |record| {
                            let _ = &cancel.reason;
                            let _ = record;
                            Ok(())
                        })?
                else {
                    let state = self.store.state.lock().map_err(|_| {
                        internal_error("question store lock failed", "mutex poisoned")
                    })?;
                    if state.completed.contains(&cancel.question_id) {
                        return serde_json::to_value(QuestionCancelResponse { cancelled: false })
                            .map_err(|e| internal_error("cancel serialization failed", e));
                    }
                    return serde_json::to_value(QuestionCancelResponse { cancelled: false })
                        .map_err(|e| internal_error("cancel serialization failed", e));
                };
                record
                    .sender
                    .send(QuestionReply {
                        question_id: cancel.question_id,
                        status: QuestionStatus::Cancelled,
                        choice: None,
                        free_text: None,
                    })
                    .map_err(|_| {
                        internal_error("question cancellation delivery failed", "channel closed")
                    })?;
                serde_json::to_value(QuestionCancelResponse { cancelled: true })
                    .map_err(|e| internal_error("cancel serialization failed", e))
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        json!({ "request": true, "reply": true, "cancel": true })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_capabilities::ClientCapabilitiesInfo;
    use crate::connection::AcpConnection;
    use tokio::sync::mpsc;

    fn context(connection_id: &str) -> ExtensionContext {
        ExtensionContext {
            session_id: Some("session-1".into()),
            principal: "owner-1".into(),
            connection_id: connection_id.into(),
            working_directory: None,
            client_capabilities: ClientCapabilitiesInfo::from_json(Some(json!({
                "_meta": { "anureo.dev": { "question": { "request": true } } }
            }))),
        }
    }

    #[tokio::test]
    async fn request_notifies_the_current_client_and_reply_resolves_it() {
        let (tx, mut rx) = mpsc::channel(4);
        let connection = Arc::new(AcpConnection::shell(
            "connection-1".into(),
            "owner-1".into(),
            tx,
        ));
        let connections = Arc::new(ConnectionRegistry::default());
        connections.insert(connection);
        let handler = Arc::new(QuestionHandler::with_connections(connections));
        let ctx = context("connection-1");
        let request = json!({
            "questionId": "question-1",
            "sessionId": "session-1",
            "prompt": "Choose a plan",
            "choices": [{ "value": "fast", "label": "Fast" }]
        });

        let pending_handler = handler.clone();
        let pending =
            tokio::spawn(async move { pending_handler.handle("request", request, &ctx).await });
        let outbound = rx.recv().await.expect("question notification");
        let ConnectionOutbound::ExtensionNotification { method, params } = outbound else {
            panic!("expected extension notification");
        };
        assert_eq!(method, "_anureo.dev/question/request");
        assert_eq!(params["questionId"], "question-1");

        let listed = handler
            .handle("list", json!({}), &context("connection-1"))
            .await
            .expect("question list");
        assert_eq!(listed.as_array().map(Vec::len), Some(1));

        let reply = handler
            .handle(
                "reply",
                json!({
                    "questionId": "question-1",
                    "status": "answered",
                    "choice": "fast"
                }),
                &context("connection-1"),
            )
            .await
            .expect("question reply");
        assert_eq!(reply["accepted"], true);
        let result = pending
            .await
            .expect("question task")
            .expect("question result");
        assert_eq!(result["status"], "answered");
        assert_eq!(result["choice"], "fast");
    }

    #[test]
    fn pending_question_can_move_to_replacement_connection() {
        let store = QuestionStore::new();
        let request: QuestionRequest = serde_json::from_value(json!({
            "questionId": "question-rebind",
            "sessionId": "session-1",
            "prompt": "Continue?",
            "choices": [{ "value": "yes", "label": "Yes" }]
        }))
        .expect("request");
        let (sender, _receiver) = oneshot::channel();
        store
            .insert(
                request,
                "connection-old".into(),
                Some("session-1".into()),
                sender,
            )
            .expect("insert");

        let rebound = store
            .rebind_session("session-1", "connection-new")
            .expect("rebind");
        assert_eq!(rebound.len(), 1);
        let taken = store
            .take_validated("question-rebind", &context("connection-new"), |_| Ok(()))
            .expect("take");
        assert!(taken.is_some());
    }
}
