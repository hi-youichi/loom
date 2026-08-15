use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const RECAP_NOTIFICATION_METHOD: &str = "_loomdesk.dev/session-assist/recap";
const MAX_SESSION_ID_LEN: usize = 256;
const MAX_RECAP_LEN: usize = 16 * 1024;
const MAX_SUGGESTION_LEN: usize = 2 * 1024;
const MAX_MODEL_NAME_LEN: usize = 256;

pub type SessionAssistCapability = Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionAssistRecapParams {
    pub session_id: String,
    pub recap: String,
    pub suggestions: Vec<String>,
    #[serde(with = "rfc3339_utc")]
    pub generated_at: DateTime<Utc>,
    pub model_used: String,
    pub turn_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssistMetadata {
    pub recap: String,
    pub suggestions: Vec<String>,
    #[serde(with = "rfc3339_utc")]
    pub generated_at: DateTime<Utc>,
    pub model_used: String,
    pub turn_index: u32,
}

impl SessionAssistRecapParams {
    pub fn from_value(params: Value) -> Result<Self, ExtensionError> {
        let recap: Self = serde_json::from_value(params)
            .map_err(|error| ExtensionError::invalid_params(error.to_string()))?;
        recap.validate().map_err(ExtensionError::invalid_params)?;
        Ok(recap)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        validate_text(&self.session_id, "sessionId", MAX_SESSION_ID_LEN)?;
        validate_text(&self.recap, "recap", MAX_RECAP_LEN)?;
        validate_suggestions(&self.suggestions)?;
        validate_text(&self.model_used, "modelUsed", MAX_MODEL_NAME_LEN)?;
        validate_utc(&self.generated_at)
    }

    pub fn notification(&self) -> Result<Value, ExtensionError> {
        self.validate().map_err(ExtensionError::invalid_params)?;
        serde_json::to_value(self)
            .map_err(|error| ExtensionError::invalid_params(error.to_string()))
    }

    pub fn notification_envelope(&self) -> Result<Value, ExtensionError> {
        Ok(serde_json::json!({
            "jsonrpc": "2.0",
            "method": RECAP_NOTIFICATION_METHOD,
            "params": self.notification()?
        }))
    }
}

impl AssistMetadata {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_text(&self.recap, "recap", MAX_RECAP_LEN)?;
        validate_suggestions(&self.suggestions)?;
        validate_text(&self.model_used, "modelUsed", MAX_MODEL_NAME_LEN)?;
        validate_utc(&self.generated_at)
    }
}

fn validate_text(value: &str, field: &'static str, max_len: usize) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > max_len {
        Err(field)
    } else {
        Ok(())
    }
}

fn validate_utc(value: &DateTime<Utc>) -> Result<(), &'static str> {
    let _ = value;
    Ok(())
}

fn validate_suggestions(values: &[String]) -> Result<(), &'static str> {
    if values.len() > 5 {
        return Err("suggestions");
    }
    if values
        .iter()
        .any(|value| value.trim().is_empty() || value.len() > MAX_SUGGESTION_LEN)
    {
        return Err("suggestions");
    }
    Ok(())
}

mod rfc3339_utc {
    use super::*;

    pub fn serialize<S>(value: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_rfc3339_opts(SecondsFormat::AutoSi, true))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let date = DateTime::parse_from_rfc3339(&value).map_err(D::Error::custom)?;
        if date.offset().local_minus_utc() != 0 {
            return Err(D::Error::custom("generatedAt must use a UTC offset"));
        }
        Ok(date.with_timezone(&Utc))
    }
}

pub struct SessionAssistHandler;

impl SessionAssistHandler {
    pub fn new() -> Self {
        Self
    }

    pub fn parse_recap(
        &self,
        params: Value,
        session_id: Option<&str>,
    ) -> Result<SessionAssistRecapParams, ExtensionError> {
        let recap = SessionAssistRecapParams::from_value(params)?;
        if let Some(expected_session_id) = session_id {
            if expected_session_id != recap.session_id {
                return Err(ExtensionError::invalid_params("sessionId"));
            }
        }
        Ok(recap)
    }

    pub fn metadata(&self, recap: &SessionAssistRecapParams) -> Result<Value, ExtensionError> {
        recap.validate().map_err(ExtensionError::invalid_params)?;
        let metadata = AssistMetadata {
            recap: recap.recap.clone(),
            suggestions: recap.suggestions.clone(),
            generated_at: recap.generated_at,
            model_used: recap.model_used.clone(),
            turn_index: recap.turn_index,
        };
        metadata
            .validate()
            .map_err(ExtensionError::invalid_params)?;
        serde_json::to_value(metadata)
            .map_err(|error| ExtensionError::invalid_params(error.to_string()))
    }
}

impl Default for SessionAssistHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExtensionHandler for SessionAssistHandler {
    async fn handle(
        &self,
        _method: &str,
        _params: Value,
        _ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        Err(ExtensionError::method_not_found())
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({"recap": true})
    }
}
