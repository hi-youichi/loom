use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const MAX_CONTROL_BYTES: usize = 16 * 1024;
const MAX_AUDIO_BYTES: usize = 256 * 1024;
const MAX_INPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_STREAM_DURATION: Duration = Duration::from_secs(60 * 60);
const MAX_LANGUAGE_BYTES: usize = 64;
const MAX_TRANSCRIPT_BYTES: usize = 16 * 1024;
const MAX_ALTERNATIVES: usize = 8;
const MAX_OUTPUT_FRAMES: usize = 64;
const MAX_INPUT_FRAMES: usize = 128;
const RATE_WINDOW: Duration = Duration::from_secs(1);
const MAX_AUDIO_FRAMES_PER_WINDOW: usize = 20;
const AUDIO_HEADER_VERSION: u8 = 1;
const AUDIO_ENCODING_LINEAR16: u8 = 1;
const AUDIO_ENCODING_OPUS: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DictationConfig {
    pub sample_rate: u32,
    pub encoding: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default = "default_true")]
    pub interim_results: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationControlCommand {
    Start,
    Stop,
    EndOfStream,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DictationControlPayload {
    pub command: DictationControlCommand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<DictationConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DictationFrameType {
    Audio,
    Text,
    Control,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DictationFrame {
    pub substream_id: String,
    #[serde(rename = "type")]
    pub frame_type: DictationFrameType,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DictationTranscriptAlternative {
    pub transcript: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DictationTranscriptPayload {
    pub transcript: String,
    pub is_final: bool,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<DictationTranscriptAlternative>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationSubstreamStart {
    pub substream_id: String,
    pub frame: DictationFrame,
}

pub type AudioFrame = Vec<u8>;

#[derive(Debug, Clone)]
pub struct DictationParentContext {
    pub principal: String,
    pub connection_id: String,
    pub session_id: Option<String>,
    pub bearer_token_valid: bool,
}

#[derive(Debug, Clone)]
pub struct DictationProviderResult {
    pub transcript: String,
    pub confidence: f32,
    pub alternatives: Vec<DictationTranscriptAlternative>,
}

#[async_trait]
pub trait DictationProvider: Send + Sync {
    fn available(&self) -> bool;

    async fn recognize(
        &self,
        audio: &[u8],
        config: &DictationConfig,
        final_result: bool,
    ) -> Result<DictationProviderResult, String>;
}

#[async_trait]
pub trait DictationPersistence: Send + Sync {
    async fn persist(&self, session_id: &str, transcript: &str) -> Result<(), String>;
}

struct DefaultDictationProvider;

#[async_trait]
impl DictationProvider for DefaultDictationProvider {
    fn available(&self) -> bool {
        true
    }

    async fn recognize(
        &self,
        audio: &[u8],
        _config: &DictationConfig,
        _final_result: bool,
    ) -> Result<DictationProviderResult, String> {
        let transcript = String::from_utf8_lossy(audio).trim().to_owned();
        Ok(DictationProviderResult {
            transcript,
            confidence: 1.0,
            alternatives: Vec::new(),
        })
    }
}

struct NoopDictationPersistence;

#[async_trait]
impl DictationPersistence for NoopDictationPersistence {
    async fn persist(&self, _session_id: &str, _transcript: &str) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationSubstreamState {
    Active,
    Stopped,
    Finalizing,
    Failed,
    Closed,
}

struct Substream {
    #[allow(dead_code)]
    id: String,
    state: DictationSubstreamState,
    config: DictationConfig,
    started: Instant,
    window_started: Instant,
    frames_in_window: usize,
    input_bytes: usize,
    audio: Vec<u8>,
    output: Vec<DictationFrame>,
    session_id: Option<String>,
    #[allow(dead_code)]
    principal: String,
    #[allow(dead_code)]
    connection_id: String,
}

pub struct DictationHandler {
    provider: Arc<dyn DictationProvider>,
    persistence: Arc<dyn DictationPersistence>,
    streams: Mutex<HashMap<String, Substream>>,
}

impl DictationHandler {
    pub fn new() -> Self {
        Self::with_dependencies(
            Arc::new(DefaultDictationProvider),
            Arc::new(NoopDictationPersistence),
        )
    }

    pub fn with_dependencies(
        provider: Arc<dyn DictationProvider>,
        persistence: Arc<dyn DictationPersistence>,
    ) -> Self {
        Self {
            provider,
            persistence,
            streams: Mutex::new(HashMap::new()),
        }
    }

    fn internal(message: impl Into<String>) -> ExtensionError {
        ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(message.into())),
        }
    }

    fn validate_config(config: &DictationConfig) -> Result<(), ExtensionError> {
        let supported_rate = matches!(config.sample_rate, 8000 | 16000 | 24000 | 48000);
        let supported_encoding = matches!(config.encoding.as_str(), "linear16" | "opus");
        if !supported_rate || !supported_encoding {
            return Err(ExtensionError::invalid_params("audio format not supported"));
        }
        if config
            .language
            .as_ref()
            .is_some_and(|v| v.trim().is_empty() || v.len() > MAX_LANGUAGE_BYTES || !v.is_ascii())
        {
            return Err(ExtensionError::invalid_params("language is invalid"));
        }
        Ok(())
    }

    fn validate_substream_id(id: &str) -> Result<(), ExtensionError> {
        if id.is_empty()
            || id.len() > 128
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(ExtensionError::invalid_params("substreamId is invalid"));
        }
        Ok(())
    }

    fn bind_session(
        url_session_id: Option<&str>,
        parent: &DictationParentContext,
    ) -> Result<Option<String>, Value> {
        if !parent.bearer_token_valid {
            return Err(Self::control_error(
                "authentication_failed",
                "Authentication failed",
                None,
            ));
        }
        if parent.principal.trim().is_empty() || parent.connection_id.trim().is_empty() {
            return Err(Self::control_error(
                "authentication_failed",
                "Authentication failed",
                None,
            ));
        }
        if url_session_id.is_some_and(|id| id.trim().is_empty() || id.len() > 256) {
            return Err(Self::control_error(
                "session_not_found",
                "Session not found",
                None,
            ));
        }
        if let (Some(url), Some(context)) = (url_session_id, parent.session_id.as_deref()) {
            if url != context {
                return Err(Self::control_error(
                    "session_not_found",
                    "Session not found",
                    None,
                ));
            }
        }
        if url_session_id.is_none() && parent.session_id.is_none() {
            return Err(Self::control_error(
                "session_not_found",
                "Session not found",
                None,
            ));
        }
        Ok(url_session_id
            .map(str::to_owned)
            .or_else(|| parent.session_id.clone()))
    }

    fn control_error(code: &str, message: &str, substream_id: Option<&str>) -> Value {
        serde_json::json!({
            "substreamId": substream_id.unwrap_or(""),
            "type": "control",
            "payload": { "command": "error", "code": code, "rpcCode": -32602, "message": message }
        })
    }

    fn control_frame(
        id: &str,
        payload: DictationControlPayload,
    ) -> Result<DictationFrame, ExtensionError> {
        Ok(DictationFrame {
            substream_id: id.to_owned(),
            frame_type: DictationFrameType::Control,
            payload: serde_json::to_value(payload)
                .map_err(|_| Self::internal("control serialization failed"))?,
        })
    }

    fn output_frame(
        id: &str,
        result: DictationProviderResult,
        is_final: bool,
    ) -> Result<DictationFrame, ExtensionError> {
        if result.transcript.len() > MAX_TRANSCRIPT_BYTES
            || result.alternatives.len() > MAX_ALTERNATIVES
        {
            return Err(Self::internal("recognizer output exceeds limits"));
        }
        if !result.confidence.is_finite() || !(0.0..=1.0).contains(&result.confidence) {
            return Err(Self::internal("recognizer returned invalid confidence"));
        }
        Ok(DictationFrame {
            substream_id: id.to_owned(),
            frame_type: DictationFrameType::Text,
            payload: serde_json::to_value(DictationTranscriptPayload {
                transcript: result.transcript,
                is_final,
                confidence: result.confidence,
                alternatives: result.alternatives,
            })
            .map_err(|_| Self::internal("transcript serialization failed"))?,
        })
    }

    pub fn open_substream(
        &self,
        url_session_id: Option<&str>,
        parent: &DictationParentContext,
        config: DictationConfig,
    ) -> Result<DictationSubstreamStart, Value> {
        let session_id = Self::bind_session(url_session_id, parent)?;
        if Self::validate_config(&config).is_err() {
            return Err(Self::control_error(
                "audio_format_not_supported",
                "Audio format not supported",
                None,
            ));
        }
        if !self.provider.available() {
            return Err(Self::control_error(
                "provider_unavailable",
                "Internal Error",
                None,
            ));
        }
        let id = format!("dict-{}", uuid::Uuid::new_v4());
        let payload = DictationControlPayload {
            command: DictationControlCommand::Start,
            config: Some(config.clone()),
            message: None,
        };
        let frame = Self::control_frame(&id, payload)
            .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(&id)))?;
        let mut streams = self
            .streams
            .lock()
            .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(&id)))?;
        streams.insert(
            id.clone(),
            Substream {
                id: id.clone(),
                state: DictationSubstreamState::Active,
                config,
                started: Instant::now(),
                window_started: Instant::now(),
                frames_in_window: 0,
                input_bytes: 0,
                output: Vec::new(),
                audio: Vec::new(),
                session_id,
                principal: parent.principal.clone(),
                connection_id: parent.connection_id.clone(),
            },
        );
        Ok(DictationSubstreamStart {
            substream_id: id,
            frame,
        })
    }

    pub async fn receive_control(&self, frame: Value) -> Result<Vec<Value>, Value> {
        if serde_json::to_vec(&frame).map_or(true, |v| v.len() > MAX_CONTROL_BYTES) {
            return Err(Self::control_error(
                "invalid_params",
                "Invalid Params",
                None,
            ));
        }
        let frame: DictationFrame = serde_json::from_value(frame)
            .map_err(|_| Self::control_error("invalid_params", "Invalid Params", None))?;
        Self::validate_substream_id(&frame.substream_id).map_err(|_| {
            Self::control_error(
                "invalid_params",
                "Invalid Params",
                Some(&frame.substream_id),
            )
        })?;
        if !matches!(&frame.frame_type, DictationFrameType::Control) {
            return Err(Self::control_error(
                "invalid_params",
                "Invalid Params",
                Some(&frame.substream_id),
            ));
        }
        let payload: DictationControlPayload =
            serde_json::from_value(frame.payload).map_err(|_| {
                Self::control_error(
                    "invalid_params",
                    "Invalid Params",
                    Some(&frame.substream_id),
                )
            })?;
        match payload.command {
            DictationControlCommand::Start => self.start(&frame.substream_id, payload.config).await,
            DictationControlCommand::Stop => self.stop(&frame.substream_id).await,
            DictationControlCommand::EndOfStream => self.end_of_stream(&frame.substream_id).await,
            DictationControlCommand::Error => self.fail(&frame.substream_id).await,
        }
    }

    async fn start(&self, id: &str, config: Option<DictationConfig>) -> Result<Vec<Value>, Value> {
        let mut streams = self
            .streams
            .lock()
            .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
        let stream = streams.get_mut(id).ok_or_else(|| {
            Self::control_error("session_not_found", "Session not found", Some(id))
        })?;
        if stream.state != DictationSubstreamState::Stopped || config.is_none() {
            return Err(Self::control_error(
                "invalid_params",
                "Invalid Params",
                Some(id),
            ));
        }
        let config = config.unwrap();
        Self::validate_config(&config).map_err(|_| {
            Self::control_error(
                "audio_format_not_supported",
                "Audio format not supported",
                Some(id),
            )
        })?;
        stream.config = config.clone();
        stream.state = DictationSubstreamState::Active;
        let frame = Self::control_frame(
            id,
            DictationControlPayload {
                command: DictationControlCommand::Start,
                config: Some(config),
                message: None,
            },
        )
        .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
        Ok(vec![serde_json::to_value(frame).map_err(|_| {
            Self::control_error("internal_error", "Internal Error", Some(id))
        })?])
    }

    async fn stop(&self, id: &str) -> Result<Vec<Value>, Value> {
        let mut streams = self
            .streams
            .lock()
            .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
        let stream = streams.get_mut(id).ok_or_else(|| {
            Self::control_error("session_not_found", "Session not found", Some(id))
        })?;
        if stream.state != DictationSubstreamState::Active {
            return Err(Self::control_error(
                "invalid_params",
                "Invalid Params",
                Some(id),
            ));
        }
        stream.state = DictationSubstreamState::Stopped;
        Ok(Vec::new())
    }

    async fn end_of_stream(&self, id: &str) -> Result<Vec<Value>, Value> {
        let (audio, config, session_id) = {
            let mut streams = self
                .streams
                .lock()
                .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
            let stream = streams.get_mut(id).ok_or_else(|| {
                Self::control_error("session_not_found", "Session not found", Some(id))
            })?;
            if stream.state != DictationSubstreamState::Active {
                return Err(Self::control_error(
                    "invalid_params",
                    "Invalid Params",
                    Some(id),
                ));
            }
            stream.state = DictationSubstreamState::Finalizing;
            (
                std::mem::take(&mut stream.audio),
                stream.config.clone(),
                stream.session_id.clone(),
            )
        };
        let result = self
            .provider
            .recognize(&audio, &config, true)
            .await
            .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
        let transcript = result.transcript.clone();
        if let Some(session_id) = session_id.as_deref() {
            self.persistence
                .persist(session_id, &transcript)
                .await
                .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
        }
        let text = Self::output_frame(id, result, true)
            .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
        let completion = Self::control_frame(
            id,
            DictationControlPayload {
                command: DictationControlCommand::EndOfStream,
                config: None,
                message: None,
            },
        )
        .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
        let mut streams = self
            .streams
            .lock()
            .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
        if let Some(stream) = streams.get_mut(id) {
            stream.state = DictationSubstreamState::Closed;
        }
        Ok(vec![
            serde_json::to_value(text).unwrap(),
            serde_json::to_value(completion).unwrap(),
        ])
    }

    async fn fail(&self, id: &str) -> Result<Vec<Value>, Value> {
        let mut streams = self
            .streams
            .lock()
            .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
        let stream = streams.get_mut(id).ok_or_else(|| {
            Self::control_error("session_not_found", "Session not found", Some(id))
        })?;
        stream.state = DictationSubstreamState::Failed;
        Ok(vec![Self::control_error(
            "client_error",
            "Client error",
            Some(id),
        )])
    }

    pub async fn receive_audio(&self, id: &str, bytes: &[u8]) -> Result<Vec<Value>, Value> {
        if bytes.len() < 2 || bytes.len() > MAX_AUDIO_BYTES {
            return Err(Self::control_error(
                "invalid_params",
                "Invalid Params",
                Some(id),
            ));
        }
        let (config, audio) = {
            let mut streams = self
                .streams
                .lock()
                .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
            let stream = streams.get_mut(id).ok_or_else(|| {
                Self::control_error("session_not_found", "Session not found", Some(id))
            })?;
            if stream.state != DictationSubstreamState::Active {
                return Err(Self::control_error(
                    "invalid_params",
                    "Invalid Params",
                    Some(id),
                ));
            }
            if stream.started.elapsed() > MAX_STREAM_DURATION {
                stream.state = DictationSubstreamState::Closed;
                return Err(Self::control_error(
                    "rate_limit_exceeded",
                    "Rate limit exceeded",
                    Some(id),
                ));
            }
            if stream.window_started.elapsed() >= RATE_WINDOW {
                stream.window_started = Instant::now();
                stream.frames_in_window = 0;
            }
            stream.frames_in_window += 1;
            if stream.frames_in_window > MAX_AUDIO_FRAMES_PER_WINDOW {
                return Err(Self::control_error(
                    "rate_limit_exceeded",
                    "Rate limit exceeded",
                    Some(id),
                ));
            }
            let encoding = if stream.config.encoding == "linear16" {
                AUDIO_ENCODING_LINEAR16
            } else {
                AUDIO_ENCODING_OPUS
            };
            if bytes[0] != AUDIO_HEADER_VERSION || bytes[1] != encoding {
                return Err(Self::control_error(
                    "audio_format_not_supported",
                    "Audio format not supported",
                    Some(id),
                ));
            }
            if stream.input_bytes + bytes.len() - 2 > MAX_INPUT_BYTES
                || stream.input_bytes / 2 >= MAX_INPUT_FRAMES
            {
                return Err(Self::control_error(
                    "backpressure",
                    "Backpressure",
                    Some(id),
                ));
            }
            stream.input_bytes += bytes.len() - 2;
            stream.audio.extend_from_slice(&bytes[2..]);
            (stream.config.clone(), bytes[2..].to_vec())
        };
        let result = self
            .provider
            .recognize(&audio, &config, false)
            .await
            .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
        if !config.interim_results {
            return Ok(Vec::new());
        }
        let frame = Self::output_frame(id, result, false)
            .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
        let mut streams = self
            .streams
            .lock()
            .map_err(|_| Self::control_error("internal_error", "Internal Error", Some(id)))?;
        let stream = streams.get_mut(id).ok_or_else(|| {
            Self::control_error("session_not_found", "Session not found", Some(id))
        })?;
        if stream.output.len() >= MAX_OUTPUT_FRAMES {
            stream
                .output
                .retain(|f| !matches!(f.frame_type, DictationFrameType::Text));
        }
        stream.output.push(frame.clone());
        Ok(vec![serde_json::to_value(frame).unwrap()])
    }
}

#[async_trait]
impl ExtensionHandler for DictationHandler {
    async fn handle(
        &self,
        _method: &str,
        _params: Value,
        _ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        Err(ExtensionError::method_not_found())
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({ "stream": true })
    }
}

impl Default for DictationHandler {
    fn default() -> Self {
        Self::new()
    }
}

fn default_true() -> bool {
    true
}
fn default_confidence() -> f32 {
    1.0
}
