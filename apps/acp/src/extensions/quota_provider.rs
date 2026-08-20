use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Value};

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

fn internal(message: impl Into<String>) -> ExtensionError {
    ExtensionError {
        code: -32603,
        message: "internal_error".into(),
        data: Some(Value::String(message.into())),
    }
}

fn param_str(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn require_param(params: &Value, key: &str) -> Result<String, ExtensionError> {
    param_str(params, key)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            ExtensionError::invalid_params(format!("missing required parameter: {key}"))
        })
}

/// Provider registry. `env_key` providers light up automatically when the
/// corresponding environment variable is set; the manual trio
/// (opencode-go / ollama-cloud / cursor) reads credentials from the store
/// file, mirroring the Express `quota/` module.
struct ProviderDef {
    id: &'static str,
    label: &'static str,
    kind: ProviderKind,
    env_key: Option<&'static str>,
}

enum ProviderKind {
    EnvKey,
    OpenCodeGo,
    OllamaCloud,
    Cursor,
}

fn providers() -> Vec<ProviderDef> {
    vec![
        ProviderDef { id: "claude", label: "Claude Pro/Max", kind: ProviderKind::EnvKey, env_key: Some("CLAUDE_ACCESS_TOKEN") },
        ProviderDef { id: "codex", label: "ChatGPT Codex", kind: ProviderKind::EnvKey, env_key: Some("CODEX_API_KEY") },
        ProviderDef { id: "zhipu", label: "Zhipu Coding Plan", kind: ProviderKind::EnvKey, env_key: Some("ZHIPU_API_KEY") },
        ProviderDef { id: "opencode-go", label: "opencode Go", kind: ProviderKind::OpenCodeGo, env_key: None },
        ProviderDef { id: "ollama-cloud", label: "Ollama Cloud", kind: ProviderKind::OllamaCloud, env_key: None },
        ProviderDef { id: "cursor", label: "Cursor", kind: ProviderKind::Cursor, env_key: None },
    ]
}

fn quota_store_path() -> Result<PathBuf, ExtensionError> {
    let home = super::config_store::user_home().map_err(|e| internal(e.message))?;
    Ok(home.join(".config").join("loomdesk").join("quota"))
}

fn load_credentials() -> Result<HashMap<String, Value>, ExtensionError> {
    let path = quota_store_path()?.join("credentials.json");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Ok(HashMap::new());
    };
    let value: Value = serde_json::from_str(&content)
        .map_err(|e| internal(format!("failed to parse credentials store: {e}")))?;
    let mut out = HashMap::new();
    if let Value::Object(map) = value {
        for (k, v) in map {
            out.insert(k, v);
        }
    }
    Ok(out)
}

fn save_credentials(map: &HashMap<String, Value>) -> Result<(), ExtensionError> {
    let dir = quota_store_path()?;
    std::fs::create_dir_all(&dir).map_err(|e| internal(e.to_string()))?;
    let path = dir.join("credentials.json");
    if path.exists() {
        // 0600-equivalent on unix; Windows ACLs default to user-only for
        // user-profile paths.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }
    let body = serde_json::to_string_pretty(&Value::Object(
        map.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    ))
    .map_err(|e| internal(e.to_string()))?;
    std::fs::write(&path, body).map_err(|e| internal(e.to_string()))
}

fn provider_configured(def: &ProviderDef, creds: &HashMap<String, Value>) -> bool {
    match (&def.kind, def.env_key) {
        (ProviderKind::EnvKey, Some(key)) => std::env::var(key)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false),
        _ => creds.contains_key(def.id),
    }
}

fn provider_credential(def: &ProviderDef, creds: &HashMap<String, Value>) -> Option<String> {
    match (&def.kind, def.env_key) {
        (ProviderKind::EnvKey, Some(key)) => std::env::var(key).ok(),
        _ => creds.get(def.id).and_then(|v| {
            v.get("token")
                .or_else(|| v.get("cookie"))
                .or_else(|| v.get("apiKey"))
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        }),
    }
}

fn provider_json(def: &ProviderDef, creds: &HashMap<String, Value>) -> Value {
    json!({
        "id": def.id,
        "label": def.label,
        "configured": provider_configured(def, creds),
    })
}

struct QuotaCache {
    fetched_at: Instant,
    body: Value,
}

pub struct QuotaProviderHandler {
    cache: Mutex<HashMap<String, QuotaCache>>,
}

impl Default for QuotaProviderHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl QuotaProviderHandler {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
        }
    }

    async fn fetch_opencode_go(token: &str) -> Result<Value, String> {
        let client = reqwest::Client::new();
        let response = client
            .post("https://api.opencode.ai/quota/check")
            .bearer_auth(token)
            .header("User-Agent", "loom-acp")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("opencode-go returned {}", response.status()));
        }
        response.json::<Value>().await.map_err(|e| e.to_string())
    }

    async fn fetch_ollama_cloud(token: &str) -> Result<Value, String> {
        let client = reqwest::Client::new();
        let response = client
            .get("https://api.ollama.com/v1/quota")
            .bearer_auth(token)
            .header("User-Agent", "loom-acp")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("ollama-cloud returned {}", response.status()));
        }
        response.json::<Value>().await.map_err(|e| e.to_string())
    }

    async fn fetch_cursor(cookie: &str) -> Result<Value, String> {
        let client = reqwest::Client::new();
        let response = client
            .post("https://api2.cursor.sh/aiserver.v1.AiService/GetUsage")
            .header("Cookie", format!("WorkosCursorSessionToken={cookie}"))
            .header("User-Agent", "loom-acp")
            .header("Content-Type", "application/json")
            .body("{}")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("cursor returned {}", response.status()));
        }
        response.json::<Value>().await.map_err(|e| e.to_string())
    }

    async fn fetch_provider(
        &self,
        def: &ProviderDef,
        credential: &str,
    ) -> Result<Value, String> {
        match def.kind {
            ProviderKind::OpenCodeGo => Self::fetch_opencode_go(credential).await,
            ProviderKind::OllamaCloud => Self::fetch_ollama_cloud(credential).await,
            ProviderKind::Cursor => Self::fetch_cursor(credential).await,
            // Env-key providers (claude/codex/zhipu) have no public usage
            // endpoint exposed by the Express migration either.
            ProviderKind::EnvKey => Ok(json!({
                "configured": true,
                "available": "unknown",
                "usedPercent": Value::Null,
                "note": "usage endpoint not published by provider",
            })),
        }
    }
}

#[async_trait::async_trait]
impl ExtensionHandler for QuotaProviderHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        _ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "providers" => {
                let creds = load_credentials()?;
                Ok(json!({
                    "providers": providers().iter().map(|p| provider_json(p, &creds)).collect::<Vec<_>>(),
                }))
            }
            "credentials_get" => {
                let id = require_param(&params, "id")?;
                let creds = load_credentials()?;
                Ok(json!({
                    "id": id,
                    "configured": creds.contains_key(&id),
                    "credential": creds.get(&id).cloned().unwrap_or(Value::Null),
                }))
            }
            "credentials_set" => {
                let id = require_param(&params, "id")?;
                if providers().iter().all(|p| p.id != id) {
                    return Err(ExtensionError::not_found(format!(
                        "unknown provider: {id}"
                    )));
                }
                let credential = params
                    .get("credential")
                    .cloned()
                    .ok_or_else(|| ExtensionError::invalid_params("credential is required"))?;
                let mut creds = load_credentials()?;
                creds.insert(id.clone(), credential);
                save_credentials(&creds)?;
                if let Ok(mut cache) = self.cache.lock() {
                    cache.remove(&id);
                }
                Ok(json!({ "success": true, "id": id }))
            }
            "credentials_delete" => {
                let id = require_param(&params, "id")?;
                let mut creds = load_credentials()?;
                let removed = creds.remove(&id).is_some();
                save_credentials(&creds)?;
                if let Ok(mut cache) = self.cache.lock() {
                    cache.remove(&id);
                }
                Ok(json!({ "success": true, "removed": removed }))
            }
            "fetch" => {
                let id = require_param(&params, "id")?;
                let def = providers()
                    .into_iter()
                    .find(|p| p.id == id)
                    .ok_or_else(|| ExtensionError::not_found(format!("unknown provider: {id}")))?;
                let creds = load_credentials()?;
                if !provider_configured(&def, &creds) {
                    return Ok(json!({
                        "id": id,
                        "configured": false,
                        "error": "not_configured",
                    }));
                }
                {
                    let cache = self.cache.lock().map_err(|e| internal(e.to_string()))?;
                    if let Some(entry) = cache.get(&id) {
                        if entry.fetched_at.elapsed().as_secs() < 300 {
                            return Ok(entry.body.clone());
                        }
                    }
                }
                let credential = provider_credential(&def, &creds).unwrap_or_default();
                let body = match self.fetch_provider(&def, &credential).await {
                    Ok(mut value) => {
                        if let Value::Object(map) = &mut value {
                            map.insert("id".into(), json!(id));
                            map.insert("configured".into(), json!(true));
                            map.insert("fetchedAt".into(), json!(chrono_now_ms()));
                        }
                        value
                    }
                    Err(message) => json!({
                        "id": id,
                        "configured": true,
                        "error": message,
                    }),
                };
                if let Ok(mut cache) = self.cache.lock() {
                    cache.insert(
                        id.clone(),
                        QuotaCache {
                            fetched_at: Instant::now(),
                            body: body.clone(),
                        },
                    );
                }
                Ok(body)
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        json!({
            "providers": true,
            "credentials_get": true,
            "credentials_set": true,
            "credentials_delete": true,
            "fetch": true,
        })
    }
}

fn chrono_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
