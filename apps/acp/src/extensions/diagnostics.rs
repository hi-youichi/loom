use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::pagination::{encode_cursor, PaginationParams};
use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;
const EXPORT_TTL_MINUTES: i64 = 30;
const COMPONENTS: [&str; 6] = ["acp", "server", "session", "mcp", "llm", "tool"];
const MAX_SEARCH_LENGTH: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn rank(&self) -> u8 {
        match self {
            Self::Trace => 0,
            Self::Debug => 1,
            Self::Info => 2,
            Self::Warn => 3,
            Self::Error => 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub component: String,
    pub message: String,
    pub details: Option<Value>,
    pub session_id: Option<String>,
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredLogEntry {
    pub id: String,
    pub entry: LogEntry,
}

pub trait DiagnosticsLogRepository: Send + Sync {
    fn query(&self, request: &DiagnosticsLogsRequest) -> Result<Vec<StoredLogEntry>, String>;
}

pub trait DiagnosticsAuthorizer: Send + Sync {
    fn authorize(&self, principal: &str, connection_id: &str) -> bool;
}

pub trait DiagnosticsArtifactStore: Send + Sync {
    fn put(
        &self,
        export_id: &str,
        principal: &str,
        connection_id: &str,
        bytes: Vec<u8>,
        expires_at: DateTime<Utc>,
    ) -> Result<u64, String>;
    fn revoke(&self, export_id: &str);
    fn url(&self, export_id: &str) -> Result<String, String>;
}

pub trait DiagnosticsProgressNotifier: Send + Sync {
    fn notify(&self, params: Value) -> Result<(), String>;
}

pub trait DiagnosticsSessionRepository: Send + Sync {
    fn count_for_principal(&self, principal: &str, session_id: Option<&str>)
        -> Result<u32, String>;
}

#[derive(Default)]
pub struct MemoryDiagnosticsLogRepository {
    pub entries: Mutex<Vec<StoredLogEntry>>,
}
impl DiagnosticsLogRepository for MemoryDiagnosticsLogRepository {
    fn query(&self, request: &DiagnosticsLogsRequest) -> Result<Vec<StoredLogEntry>, String> {
        let mut values = self
            .entries
            .lock()
            .map_err(|_| "log repository unavailable".to_string())?
            .clone();
        values.sort_by(|a, b| {
            b.entry
                .timestamp
                .cmp(&a.entry.timestamp)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(values
            .into_iter()
            .filter(|v| request.matches(&v.entry))
            .collect())
    }
}

type StoredArtifact = (String, String, Vec<u8>, DateTime<Utc>);

#[derive(Default)]
struct MemoryArtifactStore {
    values: Mutex<HashMap<String, StoredArtifact>>,
}
impl DiagnosticsArtifactStore for MemoryArtifactStore {
    fn put(
        &self,
        id: &str,
        principal: &str,
        connection_id: &str,
        bytes: Vec<u8>,
        expires_at: DateTime<Utc>,
    ) -> Result<u64, String> {
        let size = bytes.len() as u64;
        self.values
            .lock()
            .map_err(|_| "artifact store unavailable".to_string())?
            .insert(
                id.into(),
                (principal.into(), connection_id.into(), bytes, expires_at),
            );
        Ok(size)
    }
    fn revoke(&self, id: &str) {
        if let Ok(mut values) = self.values.lock() {
            values.remove(id);
        }
    }
    fn url(&self, id: &str) -> Result<String, String> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| "artifact store unavailable".to_string())?;
        let expired = values
            .get(id)
            .is_some_and(|(_, _, _, expires_at)| *expires_at <= Utc::now());
        if expired {
            values.remove(id);
        }
        if values.contains_key(id) {
            Ok(format!("/api/diagnostics/download/{id}"))
        } else {
            Err("artifact unavailable".into())
        }
    }
}

struct DefaultAuthorizer;
impl DiagnosticsAuthorizer for DefaultAuthorizer {
    fn authorize(&self, principal: &str, _: &str) -> bool {
        !principal.trim().is_empty()
    }
}
struct NoopNotifier;
impl DiagnosticsProgressNotifier for NoopNotifier {
    fn notify(&self, _: Value) -> Result<(), String> {
        Ok(())
    }
}
struct DefaultSessions;
impl DiagnosticsSessionRepository for DefaultSessions {
    fn count_for_principal(&self, _: &str, session_id: Option<&str>) -> Result<u32, String> {
        Ok(u32::from(session_id.is_some()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticsLogsRequest {
    pub level: Option<LogLevel>,
    pub component: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub search: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticsExportRequest {
    #[serde(default = "yes")]
    pub include_logs: bool,
    #[serde(default = "yes")]
    pub include_config: bool,
    #[serde(default = "yes")]
    pub include_session_metadata: bool,
    #[serde(default = "yes")]
    pub include_system_info: bool,
    pub log_level: Option<LogLevel>,
    pub since: Option<DateTime<Utc>>,
    pub format: Option<ExportFormat>,
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Json,
    Text,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogCursor {
    pub timestamp: DateTime<Utc>,
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportContents {
    pub logs: Option<ExportLogSummary>,
    pub config: Option<ExportConfigSummary>,
    pub session_metadata: Option<ExportSessionSummary>,
    pub system_info: Option<ExportSystemInfo>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportLogSummary {
    pub entry_count: u64,
    pub time_range: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportConfigSummary {
    pub version: String,
    pub features: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSessionSummary {
    pub session_count: u32,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSystemInfo {
    pub os: String,
    pub arch: String,
    pub rust_version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsExportResponse {
    pub export_id: String,
    pub format: ExportFormat,
    pub download_url: String,
    pub expires_at: DateTime<Utc>,
    pub size: u64,
    pub contents: ExportContents,
    pub redacted: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsProgressNotification {
    pub operation_id: String,
    pub progress: u8,
    pub phase: DiagnosticsProgressPhase,
    pub message: String,
    pub cancelable: bool,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsProgressPhase {
    CollectingLogs,
    CollectingConfig,
    CollectingMetadata,
    Redacting,
    Packaging,
}

pub struct DiagnosticsHandler {
    logs: Arc<dyn DiagnosticsLogRepository>,
    authorizer: Arc<dyn DiagnosticsAuthorizer>,
    artifacts: Arc<dyn DiagnosticsArtifactStore>,
    notifier: Arc<dyn DiagnosticsProgressNotifier>,
    sessions: Arc<dyn DiagnosticsSessionRepository>,
    cancelled: Arc<Mutex<HashSet<String>>>,
}

impl DiagnosticsHandler {
    pub fn new() -> Self {
        Self::with_dependencies(
            Arc::new(MemoryDiagnosticsLogRepository::default()),
            Arc::new(DefaultAuthorizer),
            Arc::new(MemoryArtifactStore::default()),
            Arc::new(NoopNotifier),
            Arc::new(DefaultSessions),
        )
    }
    pub fn with_dependencies(
        logs: Arc<dyn DiagnosticsLogRepository>,
        authorizer: Arc<dyn DiagnosticsAuthorizer>,
        artifacts: Arc<dyn DiagnosticsArtifactStore>,
        notifier: Arc<dyn DiagnosticsProgressNotifier>,
        sessions: Arc<dyn DiagnosticsSessionRepository>,
    ) -> Self {
        Self {
            logs,
            authorizer,
            artifacts,
            notifier,
            sessions,
            cancelled: Arc::new(Mutex::new(HashSet::new())),
        }
    }
    pub fn cancel_operation(&self, operation_id: &str) {
        if let Ok(mut cancelled) = self.cancelled.lock() {
            cancelled.insert(operation_id.into());
        }
    }
    fn internal() -> ExtensionError {
        ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String("diagnostics operation failed".into())),
        }
    }
    fn parse<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
        if !params.is_object() {
            return Err(ExtensionError::invalid_params("params must be an object"));
        }
        serde_json::from_value(params)
            .map_err(|_| ExtensionError::invalid_params("invalid parameters"))
    }
    fn check_limit(value: &Value) -> Result<(), ExtensionError> {
        if let Some(v) = value.get("limit") {
            let n = v
                .as_u64()
                .ok_or_else(|| ExtensionError::invalid_params("limit must be an integer"))?;
            if n == 0 || n > MAX_LIMIT as u64 {
                return Err(ExtensionError::invalid_params(
                    "limit is outside the allowed range",
                ));
            }
        }
        Ok(())
    }
    fn check_search(value: &Value) -> Result<(), ExtensionError> {
        if let Some(search) = value.get("search").and_then(Value::as_str) {
            if search.chars().count() > MAX_SEARCH_LENGTH {
                return Err(ExtensionError::invalid_params("search is too long"));
            }
        }
        Ok(())
    }
    fn cancelled(&self, id: &str) -> bool {
        self.cancelled
            .lock()
            .map(|v| v.contains(id))
            .unwrap_or(true)
    }
    fn progress(
        &self,
        id: &str,
        progress: u8,
        phase: DiagnosticsProgressPhase,
        message: &str,
    ) -> Result<(), ExtensionError> {
        self.notifier
            .notify(
                serde_json::to_value(DiagnosticsProgressNotification {
                    operation_id: id.into(),
                    progress,
                    phase,
                    message: message.into(),
                    cancelable: true,
                })
                .map_err(|_| Self::internal())?,
            )
            .map_err(|_| Self::internal())
    }
    fn capability(&self, method: &str, ctx: &ExtensionContext) -> Result<(), ExtensionError> {
        let _ = ctx;
        if method != "logs" && method != "export" && method != "info" {
            return Err(ExtensionError::method_not_found());
        }
        Ok(())
    }
    fn info(&self) -> Result<Value, ExtensionError> {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let start = START.get_or_init(std::time::Instant::now);
        Ok(serde_json::json!({
            "AnureoVersion": env!("CARGO_PKG_VERSION"),
            "runtime": "anureo",
            "pid": std::process::id(),
            "startedAt": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
                .saturating_sub(start.elapsed().as_millis() as u64),
        }))
    }
    fn logs(&self, raw: Value) -> Result<Value, ExtensionError> {
        Self::check_limit(&raw)?;
        Self::check_search(&raw)?;
        let request: DiagnosticsLogsRequest = Self::parse(raw)?;
        if request.since.zip(request.until).is_some_and(|(a, b)| a > b) {
            return Err(ExtensionError::invalid_params(
                "since must not be later than until",
            ));
        }
        if let Some(component) = &request.component {
            if !COMPONENTS.contains(&component.as_str()) {
                return Err(ExtensionError::invalid_params("invalid component"));
            }
        }
        let pagination = PaginationParams {
            cursor: request.cursor.clone(),
            limit: request.limit,
        };
        let cursor = pagination.decode_cursor::<LogCursor>()?;
        if cursor
            .as_ref()
            .is_some_and(|value| value.id.trim().is_empty())
        {
            return Err(ExtensionError::invalid_params("invalid cursor"));
        }
        let mut entries = self.logs.query(&request).map_err(|_| Self::internal())?;
        if let Some(cursor) = cursor {
            if let Some(index) = entries
                .iter()
                .position(|v| v.entry.timestamp == cursor.timestamp && v.id == cursor.id)
            {
                entries = entries.into_iter().skip(index + 1).collect();
            } else {
                return Err(ExtensionError::invalid_params(
                    "cursor is outside the result set",
                ));
            }
        }
        let limit = pagination.limit_or_default(DEFAULT_LIMIT, MAX_LIMIT);
        let has_more = entries.len() > limit;
        let page: Vec<_> = entries.into_iter().take(limit).collect();
        let next = if has_more {
            let last = page.last().ok_or_else(Self::internal)?;
            Some(encode_cursor(
                serde_json::json!({ "timestamp": last.entry.timestamp, "id": last.id }),
            ))
        } else {
            None
        };
        let items: Vec<_> = page.into_iter().map(|v| redact_entry(v.entry)).collect();
        Ok(serde_json::json!({ "items": items, "nextCursor": next, "hasMore": has_more }))
    }
    fn export(&self, raw: Value, ctx: &ExtensionContext) -> Result<Value, ExtensionError> {
        let request: DiagnosticsExportRequest = Self::parse(raw)?;
        if !self
            .authorizer
            .authorize(&ctx.principal, &ctx.connection_id)
        {
            return Err(ExtensionError::forbidden(
                "diagnostics export authorization required",
            ));
        }
        let id = format!("diag_export_{}", Uuid::new_v4().simple());
        if self
            .progress(
                &id,
                0,
                DiagnosticsProgressPhase::CollectingLogs,
                "Collecting logs...",
            )
            .is_err()
        {
            return Err(Self::internal());
        }
        let logs = if request.include_logs {
            self.logs
                .query(&DiagnosticsLogsRequest {
                    level: request.log_level.clone(),
                    component: None,
                    since: request.since,
                    until: None,
                    search: None,
                    cursor: None,
                    limit: None,
                })
                .map_err(|_| Self::internal())?
        } else {
            vec![]
        };
        if self.cancelled(&id) {
            return Err(Self::internal());
        }
        self.progress(
            &id,
            25,
            DiagnosticsProgressPhase::CollectingConfig,
            "Collecting configuration...",
        )?;
        if self.cancelled(&id) {
            return Err(Self::internal());
        }
        self.progress(
            &id,
            45,
            DiagnosticsProgressPhase::CollectingMetadata,
            "Collecting session metadata...",
        )?;
        if self.cancelled(&id) {
            return Err(Self::internal());
        }
        self.progress(
            &id,
            65,
            DiagnosticsProgressPhase::Redacting,
            "Redacting diagnostic content...",
        )?;
        let redacted_logs: Vec<_> = logs
            .iter()
            .cloned()
            .map(|v| redact_entry(v.entry))
            .collect();
        let mut found = HashSet::new();
        for entry in &redacted_logs {
            scan_redactions(
                &serde_json::to_value(entry).map_err(|_| Self::internal())?,
                &mut found,
            );
        }
        let contents = ExportContents {
            logs: request.include_logs.then(|| ExportLogSummary {
                entry_count: redacted_logs.len() as u64,
                time_range: time_range(&redacted_logs),
            }),
            config: request.include_config.then(|| ExportConfigSummary {
                version: env!("CARGO_PKG_VERSION").into(),
                features: vec![],
            }),
            session_metadata: if request.include_session_metadata {
                Some(ExportSessionSummary {
                    session_count: self
                        .sessions
                        .count_for_principal(&ctx.principal, ctx.session_id.as_deref())
                        .map_err(|_| Self::internal())?,
                })
            } else {
                None
            },
            system_info: request.include_system_info.then(|| ExportSystemInfo {
                os: std::env::consts::OS.into(),
                arch: std::env::consts::ARCH.into(),
                rust_version: "unknown".into(),
            }),
        };
        let payload = serde_json::json!({ "contents": contents, "logs": if request.include_logs { Value::Array(redacted_logs.into_iter().filter_map(|v| serde_json::to_value(v).ok()).collect()) } else { Value::Null } });
        self.progress(
            &id,
            85,
            DiagnosticsProgressPhase::Packaging,
            "Packaging export...",
        )?;
        let bytes = match request.format.as_ref().unwrap_or(&ExportFormat::Json) {
            ExportFormat::Json => serde_json::to_vec(&payload).map_err(|_| Self::internal())?,
            ExportFormat::Text => serde_json::to_string_pretty(&payload)
                .map_err(|_| Self::internal())?
                .into_bytes(),
        };
        if self.cancelled(&id) {
            self.artifacts.revoke(&id);
            return Err(Self::internal());
        }
        let expires_at = Utc::now() + chrono::Duration::minutes(EXPORT_TTL_MINUTES);
        let size = self
            .artifacts
            .put(&id, &ctx.principal, &ctx.connection_id, bytes, expires_at)
            .map_err(|_| Self::internal())?;
        let url = match self.artifacts.url(&id) {
            Ok(url) if url.starts_with('/') && !url.contains("..") => url,
            Ok(url) => {
                self.artifacts.revoke(&id);
                return Err(ExtensionError::directory_boundary_violation(&url));
            }
            Err(_) => {
                self.artifacts.revoke(&id);
                return Err(Self::internal());
            }
        };
        if self
            .progress(
                &id,
                100,
                DiagnosticsProgressPhase::Packaging,
                "Export ready",
            )
            .is_err()
        {
            self.artifacts.revoke(&id);
            return Err(Self::internal());
        }
        serde_json::to_value(DiagnosticsExportResponse {
            export_id: id,
            format: request.format.unwrap_or(ExportFormat::Json),
            download_url: url,
            expires_at,
            size,
            contents,
            redacted: found.into_iter().collect(),
        })
        .map_err(|_| Self::internal())
    }
}

impl Default for DiagnosticsHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExtensionHandler for DiagnosticsHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        self.capability(method, ctx)?;
        match method {
            "logs" => self.logs(params),
            "export" => self.export(params, ctx),
            "info" => self.info(),
            _ => Err(ExtensionError::method_not_found()),
        }
    }
    fn capabilities(&self) -> Value {
        serde_json::json!({ "logs": true, "export": true, "info": true })
    }
}

impl DiagnosticsLogsRequest {
    fn matches(&self, entry: &LogEntry) -> bool {
        self.level
            .as_ref()
            .is_none_or(|level| entry.level.rank() >= level.rank())
            && self
                .component
                .as_ref()
                .is_none_or(|component| &entry.component == component)
            && self.since.is_none_or(|since| entry.timestamp >= since)
            && self.until.is_none_or(|until| entry.timestamp <= until)
            && self.search.as_ref().is_none_or(|search| {
                let needle = search.to_ascii_lowercase();
                entry.message.to_ascii_lowercase().contains(&needle)
                    || entry.details.as_ref().is_some_and(|details| {
                        details.to_string().to_ascii_lowercase().contains(&needle)
                    })
            })
    }
}
fn redact_entry(mut entry: LogEntry) -> LogEntry {
    entry.message = redact_string(&entry.message);
    entry.details = entry.details.map(redact_value);
    entry.session_id = entry.session_id.map(|v| redact_string(&v));
    entry.thread_id = entry.thread_id.map(|v| redact_string(&v));
    entry
}
fn redact_string(value: &str) -> String {
    let mut result = value.to_string();
    let mut output = String::with_capacity(result.len());
    let mut token = String::new();
    for ch in result.chars() {
        if ch.is_whitespace() || matches!(ch, ',' | ';' | ')' | ']' | '}') {
            if !token.is_empty() {
                output.push_str(&redact_token(&token));
                token.clear();
            }
            output.push(ch);
        } else {
            token.push(ch);
        }
    }
    if !token.is_empty() {
        output.push_str(&redact_token(&token));
    }
    result.clear();
    result.push_str(&output);
    result
}
fn redact_token(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if [
        "token",
        "secret",
        "api_key",
        "apikey",
        "password",
        "credential",
        "bearer",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || looks_like_sensitive_path(value)
        || looks_like_api_key(value)
    {
        return "****".into();
    }
    value.into()
}
fn looks_like_sensitive_path(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    (value.contains('/') || value.contains('\\'))
        && (lower.contains("/users/")
            || lower.contains("/home/")
            || lower.contains("c:\\users\\")
            || lower.contains("\\users\\")
            || lower.contains("/private/")
            || lower.contains("/var/")
            || lower.contains("/tmp/"))
}
fn looks_like_api_key(value: &str) -> bool {
    let compact = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .count();
    compact >= 24
        && value.chars().any(|ch| ch.is_ascii_uppercase())
        && value.chars().any(|ch| ch.is_ascii_digit())
}
fn redact_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let sensitive = [
                        "token",
                        "secret",
                        "api_key",
                        "apikey",
                        "password",
                        "credential",
                        "path",
                        "prompt",
                        "response",
                        "source",
                    ]
                    .iter()
                    .any(|part| key.to_ascii_lowercase().contains(part));
                    (
                        key.clone(),
                        if sensitive || looks_like_sensitive_path(&key) {
                            Value::String("****".into())
                        } else {
                            redact_value(value)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(redact_value).collect()),
        Value::String(value) => Value::String(redact_string(&value)),
        value => value,
    }
}
fn scan_redactions(value: &Value, found: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if [
                    "token",
                    "secret",
                    "api_key",
                    "apikey",
                    "password",
                    "credential",
                    "path",
                ]
                .iter()
                .any(|part| key.to_ascii_lowercase().contains(part))
                {
                    found.insert(key.to_ascii_lowercase());
                }
                scan_redactions(value, found)
            }
        }
        Value::Array(values) => {
            for value in values {
                scan_redactions(value, found)
            }
        }
        Value::String(value) if *value != redact_string(value) => {
            found.insert("sensitive_value".into());
        }
        _ => {}
    }
}
fn time_range(entries: &[LogEntry]) -> String {
    match (entries.first(), entries.last()) {
        (Some(first), Some(last)) => format!(
            "{} to {}",
            last.timestamp.to_rfc3339(),
            first.timestamp.to_rfc3339()
        ),
        _ => String::new(),
    }
}
