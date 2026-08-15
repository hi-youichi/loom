use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const SEGMENT_LENGTH: usize = 4096;
const MAX_TEXT_LENGTH: usize = 1_048_576;
const MAX_CHUNKED_AUDIO_BYTES: usize = 8 * 1024 * 1024;
const MAX_SUMMARY_LENGTH: u32 = 16_384;
const MAX_SUBSTREAM_URL_LENGTH: usize = 2048;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TtsAudioFormat {
    Mp3,
    Opus,
    Wav,
    Aac,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TtsSynthesizeParams {
    pub text: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default = "default_format")]
    pub format: TtsAudioFormat,
    #[serde(default = "default_speed")]
    pub speed: f32,
    #[serde(default = "default_true")]
    pub substream: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TtsSummarizeParams {
    pub text: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default = "default_summary_length")]
    pub max_summary_length: u32,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default = "default_format")]
    pub format: TtsAudioFormat,
    #[serde(default = "default_true")]
    pub substream: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TtsResponseMode {
    Substream,
    Chunked,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsSynthesizeResult {
    pub mode: TtsResponseMode,
    pub format: TtsAudioFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub substream_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub substream_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsSummarizeResult {
    pub summary: String,
    pub mode: TtsResponseMode,
    pub format: TtsAudioFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub substream_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub substream_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct TtsProviderOutput {
    pub audio: Vec<u8>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct TtsSubstreamDescriptor {
    pub substream_id: String,
    pub substream_url: String,
    pub principal: String,
    pub connection_id: String,
    pub session_id: Option<String>,
}

#[async_trait]
pub trait TtsProvider: Send + Sync {
    fn available(&self) -> bool;

    async fn synthesize(
        &self,
        text: &str,
        voice: Option<&str>,
        format: &TtsAudioFormat,
        speed: f32,
    ) -> Result<TtsProviderOutput, String>;
}

#[async_trait]
pub trait TtsSummarizer: Send + Sync {
    async fn summarize(&self, text: &str, max_length: u32) -> Result<String, String>;
}

struct DefaultTtsProvider;

#[async_trait]
impl TtsProvider for DefaultTtsProvider {
    fn available(&self) -> bool {
        true
    }

    async fn synthesize(
        &self,
        text: &str,
        _voice: Option<&str>,
        _format: &TtsAudioFormat,
        speed: f32,
    ) -> Result<TtsProviderOutput, String> {
        let audio = text.as_bytes().to_vec();
        let duration_ms = ((text.chars().count() as f64 / 12.0) * 1000.0 / speed as f64)
            .ceil()
            .max(1.0) as u64;
        Ok(TtsProviderOutput { audio, duration_ms })
    }
}

struct DefaultTtsSummarizer;

#[async_trait]
impl TtsSummarizer for DefaultTtsSummarizer {
    async fn summarize(&self, text: &str, max_length: u32) -> Result<String, String> {
        Ok(text.chars().take(max_length as usize).collect())
    }
}

pub struct TtsHandler {
    provider: Arc<dyn TtsProvider>,
    summarizer: Arc<dyn TtsSummarizer>,
}

impl TtsHandler {
    pub fn new() -> Self {
        Self::with_dependencies(Arc::new(DefaultTtsProvider), Arc::new(DefaultTtsSummarizer))
    }

    pub fn with_dependencies(
        provider: Arc<dyn TtsProvider>,
        summarizer: Arc<dyn TtsSummarizer>,
    ) -> Self {
        Self {
            provider,
            summarizer,
        }
    }

    fn internal(message: impl Into<String>) -> ExtensionError {
        ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: Some(Value::String(message.into())),
        }
    }

    fn parse<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
        if !params.is_object() {
            return Err(ExtensionError::invalid_params("params must be an object"));
        }
        serde_json::from_value(params).map_err(|_| ExtensionError::invalid_params("invalid params"))
    }

    fn validate_text(text: &str) -> Result<(), ExtensionError> {
        if text.trim().is_empty() {
            return Err(ExtensionError::invalid_params("text must not be empty"));
        }
        if text.len() > MAX_TEXT_LENGTH {
            return Err(ExtensionError::invalid_params(
                "text exceeds maximum length",
            ));
        }
        Ok(())
    }

    fn validate_session(
        requested: Option<&str>,
        ctx: &ExtensionContext,
    ) -> Result<Option<String>, ExtensionError> {
        if requested.is_some_and(|value| value.trim().is_empty() || value.len() > 256) {
            return Err(ExtensionError::invalid_params("sessionId is invalid"));
        }
        if let (Some(requested), Some(context)) = (requested, ctx.session_id.as_deref()) {
            if requested != context {
                return Err(ExtensionError::not_found("session not found"));
            }
        }
        Ok(requested
            .map(str::to_owned)
            .or_else(|| ctx.session_id.clone()))
    }

    fn validate_voice(voice: Option<&str>) -> Result<(), ExtensionError> {
        if voice.is_some_and(|value| value.trim().is_empty() || value.len() > 128) {
            return Err(ExtensionError::invalid_params("voice is invalid"));
        }
        Ok(())
    }

    fn validate_speed(speed: f32) -> Result<(), ExtensionError> {
        if !speed.is_finite() || !(0.25..=4.0).contains(&speed) {
            return Err(ExtensionError::invalid_params(
                "speed must be between 0.25 and 4.0",
            ));
        }
        Ok(())
    }

    fn split_text(text: &str) -> Vec<&str> {
        let mut result = Vec::new();
        let mut start = 0;
        let mut count = 0;
        for (index, character) in text.char_indices() {
            if count == SEGMENT_LENGTH {
                result.push(&text[start..index]);
                start = index;
                count = 0;
            }
            count += 1;
            let _ = character;
        }
        if start < text.len() {
            result.push(&text[start..]);
        }
        result
    }

    async fn synthesize_audio(
        &self,
        request: &TtsSynthesizeParams,
    ) -> Result<TtsProviderOutput, ExtensionError> {
        if !self.provider.available() {
            return Err(ExtensionError::capability_not_supported("tts"));
        }
        let mut audio = Vec::new();
        let mut duration_ms = 0_u64;
        for segment in Self::split_text(&request.text) {
            let output = self
                .provider
                .synthesize(
                    segment,
                    request.voice.as_deref(),
                    &request.format,
                    request.speed,
                )
                .await
                .map_err(Self::internal)?;
            if output.audio.is_empty() || output.duration_ms == 0 {
                return Err(Self::internal(
                    "provider returned empty audio or invalid duration",
                ));
            }
            audio.extend(output.audio);
            duration_ms = duration_ms
                .checked_add(output.duration_ms)
                .ok_or_else(|| Self::internal("audio duration overflow"))?;
        }
        if audio.is_empty() {
            return Err(Self::internal("provider returned no audio"));
        }
        Ok(TtsProviderOutput { audio, duration_ms })
    }

    fn descriptor(
        &self,
        ctx: &ExtensionContext,
        session_id: Option<String>,
        duration_ms: u64,
    ) -> Result<TtsSubstreamDescriptor, ExtensionError> {
        if ctx.principal.trim().is_empty() || ctx.connection_id.trim().is_empty() {
            return Err(ExtensionError::forbidden(
                "substream authorization required",
            ));
        }
        let substream_id = format!("tts-{}", uuid::Uuid::new_v4());
        let mut url = format!("/substream?type=tts&substreamId={substream_id}");
        if let Some(session_id) = session_id.as_deref() {
            url.push_str("&sessionId=");
            url.push_str(session_id);
        }
        if url.len() > MAX_SUBSTREAM_URL_LENGTH || duration_ms == 0 {
            return Err(Self::internal("invalid substream descriptor"));
        }
        Ok(TtsSubstreamDescriptor {
            substream_id,
            substream_url: url,
            principal: ctx.principal.clone(),
            connection_id: ctx.connection_id.clone(),
            session_id,
        })
    }

    fn encode(audio: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut result = String::with_capacity(audio.len().div_ceil(3) * 4);
        for chunk in audio.chunks(3) {
            let a = chunk[0] as u32;
            let b = chunk.get(1).copied().unwrap_or(0) as u32;
            let c = chunk.get(2).copied().unwrap_or(0) as u32;
            result.push(TABLE[((a >> 2) & 63) as usize] as char);
            result.push(TABLE[(((a << 4) | (b >> 4)) & 63) as usize] as char);
            result.push(if chunk.len() > 1 {
                TABLE[(((b << 2) | (c >> 6)) & 63) as usize] as char
            } else {
                '='
            });
            result.push(if chunk.len() > 2 {
                TABLE[(c & 63) as usize] as char
            } else {
                '='
            });
        }
        result
    }

    fn synthesize_result(
        &self,
        output: TtsProviderOutput,
        format: TtsAudioFormat,
        substream: bool,
        ctx: &ExtensionContext,
        session_id: Option<String>,
    ) -> Result<TtsSynthesizeResult, ExtensionError> {
        if substream {
            let descriptor = self.descriptor(ctx, session_id, output.duration_ms)?;
            Ok(TtsSynthesizeResult {
                mode: TtsResponseMode::Substream,
                format,
                substream_id: Some(descriptor.substream_id),
                substream_url: Some(descriptor.substream_url),
                estimated_duration_ms: Some(output.duration_ms),
                audio: None,
                duration_ms: None,
            })
        } else {
            if output.audio.len() > MAX_CHUNKED_AUDIO_BYTES {
                return Err(Self::internal("chunked audio exceeds maximum size"));
            }
            Ok(TtsSynthesizeResult {
                mode: TtsResponseMode::Chunked,
                format,
                substream_id: None,
                substream_url: None,
                estimated_duration_ms: None,
                audio: Some(Self::encode(&output.audio)),
                duration_ms: Some(output.duration_ms),
            })
        }
    }
}

#[async_trait]
impl ExtensionHandler for TtsHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "synthesize" => {
                let request: TtsSynthesizeParams = Self::parse(params)?;
                Self::validate_text(&request.text)?;
                Self::validate_voice(request.voice.as_deref())?;
                Self::validate_speed(request.speed)?;
                let session_id = Self::validate_session(request.session_id.as_deref(), ctx)?;
                let output = self.synthesize_audio(&request).await?;
                serde_json::to_value(self.synthesize_result(
                    output,
                    request.format,
                    request.substream,
                    ctx,
                    session_id,
                )?)
                .map_err(|_| Self::internal("failed to serialize synthesis result"))
            }
            "summarize" => {
                let request: TtsSummarizeParams = Self::parse(params)?;
                Self::validate_text(&request.text)?;
                Self::validate_voice(request.voice.as_deref())?;
                if request.max_summary_length == 0
                    || request.max_summary_length > MAX_SUMMARY_LENGTH
                {
                    return Err(ExtensionError::invalid_params(
                        "maxSummaryLength is invalid",
                    ));
                }
                let session_id = Self::validate_session(request.session_id.as_deref(), ctx)?;
                let summary = self
                    .summarizer
                    .summarize(&request.text, request.max_summary_length)
                    .await
                    .unwrap_or_else(|_| {
                        request
                            .text
                            .chars()
                            .take(request.max_summary_length as usize)
                            .collect()
                    });
                if summary.trim().is_empty()
                    || summary.chars().count() > request.max_summary_length as usize
                {
                    return Err(Self::internal("summary result is invalid"));
                }
                let synthesis_request = TtsSynthesizeParams {
                    text: summary.clone(),
                    session_id: request.session_id,
                    voice: request.voice,
                    format: request.format.clone(),
                    speed: 1.0,
                    substream: request.substream,
                };
                let output = self.synthesize_audio(&synthesis_request).await?;
                let result = self.synthesize_result(
                    output,
                    request.format,
                    request.substream,
                    ctx,
                    session_id,
                )?;
                let result = TtsSummarizeResult {
                    summary,
                    mode: result.mode,
                    format: result.format,
                    substream_id: result.substream_id,
                    substream_url: result.substream_url,
                    estimated_duration_ms: result.estimated_duration_ms,
                    audio: result.audio,
                    duration_ms: result.duration_ms,
                };
                serde_json::to_value(result)
                    .map_err(|_| Self::internal("failed to serialize summary result"))
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({ "synthesize": true, "summarize": true })
    }
}

impl Default for TtsHandler {
    fn default() -> Self {
        Self::new()
    }
}

fn default_format() -> TtsAudioFormat {
    TtsAudioFormat::Mp3
}

fn default_speed() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

fn default_summary_length() -> u32 {
    500
}
