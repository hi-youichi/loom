use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

pub const BLOCKED_PORTS: &[u16] = &[22, 25, 445, 3306, 5432, 6379, 27017];

const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_HEADERS: usize = 64;
const MAX_HEADER_BYTES: usize = 8 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewRuntime {
    Web,
    Desktop,
    VsCode,
    Cli,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewProxyRequest {
    #[serde(default = "default_method")]
    pub method: String,
    pub port: u16,
    #[serde(default = "default_path")]
    pub path: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewProxyResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub body_encoding: String,
    pub from_cache: bool,
}

pub struct PreviewHandler {
    runtime: PreviewRuntime,
    client: reqwest::Client,
}

impl PreviewHandler {
    pub fn new() -> Self {
        Self::with_runtime(PreviewRuntime::Web)
    }

    pub fn with_runtime(runtime: PreviewRuntime) -> Self {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(REQUEST_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("preview HTTP client configuration is valid");
        Self { runtime, client }
    }

    fn error(code: i32, message: &str, detail: impl Into<String>) -> ExtensionError {
        ExtensionError {
            code,
            message: message.into(),
            data: Some(Value::String(detail.into())),
        }
    }

    fn internal() -> ExtensionError {
        Self::error(-32603, "internal_error", "preview proxy failed")
    }

    fn unreachable() -> ExtensionError {
        Self::error(
            -32011,
            "proxy_target_unreachable",
            "localhost target is unreachable",
        )
    }

    fn timeout() -> ExtensionError {
        Self::error(-32010, "proxy_timeout", "preview proxy request timed out")
    }

    fn ssrf() -> ExtensionError {
        Self::error(-32012, "ssrf_blocked", "preview target is not loopback")
    }

    fn parse(params: Value) -> Result<PreviewProxyRequest, ExtensionError> {
        if !params.is_object() {
            return Err(ExtensionError::invalid_params("params must be an object"));
        }
        serde_json::from_value(params)
            .map_err(|_| ExtensionError::invalid_params("invalid preview proxy params"))
    }

    fn validate(request: &PreviewProxyRequest) -> Result<(), ExtensionError> {
        if !(1024..=u16::MAX).contains(&request.port) || BLOCKED_PORTS.contains(&request.port) {
            return Err(ExtensionError::invalid_params("port is not allowed"));
        }
        if !matches!(
            request.method.as_str(),
            "GET" | "POST" | "PUT" | "DELETE" | "HEAD" | "OPTIONS"
        ) {
            return Err(ExtensionError::invalid_params("HTTP method is not allowed"));
        }
        let path = request.path.as_str();
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains("../")
            || path.contains("..\\")
            || path.contains("\\")
            || path.contains("://")
            || path.contains('#')
            || path.chars().any(|c| c.is_control() || c == '\0')
            || path.to_ascii_lowercase().contains("%2e")
            || path.to_ascii_lowercase().contains("%2f")
            || path.to_ascii_lowercase().contains("%5c")
        {
            return Err(ExtensionError::invalid_params("path is not allowed"));
        }
        if path.as_bytes().windows(1).any(|w| w == [b'%']) {
            let bytes = path.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'%' {
                    if i + 2 >= bytes.len()
                        || !bytes[i + 1].is_ascii_hexdigit()
                        || !bytes[i + 2].is_ascii_hexdigit()
                    {
                        return Err(ExtensionError::invalid_params("path encoding is invalid"));
                    }
                    i += 3;
                } else {
                    i += 1;
                }
            }
        }
        if request
            .body
            .as_ref()
            .is_some_and(|body| body.len() > MAX_BODY_BYTES)
        {
            return Err(ExtensionError::invalid_params("request body is too large"));
        }
        if request.headers.len() > MAX_HEADERS {
            return Err(ExtensionError::invalid_params("too many request headers"));
        }
        let mut total = 0;
        for (name, value) in &request.headers {
            total += name.len() + value.len();
            if name.is_empty()
                || name.len() > 256
                || value.len() > MAX_HEADER_BYTES
                || name.chars().any(|c| c.is_control())
                || value.chars().any(|c| c.is_control())
            {
                return Err(ExtensionError::invalid_params("request header is invalid"));
            }
            if name.eq_ignore_ascii_case("host")
                || name.eq_ignore_ascii_case("origin")
                || name.eq_ignore_ascii_case("content-length")
                || name.eq_ignore_ascii_case("connection")
                || name.eq_ignore_ascii_case("forwarded")
                || name.to_ascii_lowercase().starts_with("x-forwarded-")
            {
                continue;
            }
            HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| ExtensionError::invalid_params("request header name is invalid"))?;
            HeaderValue::from_str(value)
                .map_err(|_| ExtensionError::invalid_params("request header value is invalid"))?;
        }
        if total > MAX_HEADER_BYTES * 2 {
            return Err(ExtensionError::invalid_params(
                "request headers are too large",
            ));
        }
        Ok(())
    }

    fn filtered_headers(response: &reqwest::Response) -> HashMap<String, String> {
        response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                let name = name.as_str();
                let lower = name.to_ascii_lowercase();
                if lower == "set-cookie"
                    || lower == "transfer-encoding"
                    || lower == "connection"
                    || lower == "x-powered-by"
                    || lower.starts_with("x-forwarded-")
                {
                    return None;
                }
                Some((name.to_string(), value.to_str().ok()?.to_string()))
            })
            .collect()
    }
}

fn default_method() -> String {
    "GET".into()
}
fn default_path() -> String {
    "/".into()
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0] as usize;
        let second = chunk.get(1).copied().unwrap_or(0) as usize;
        let third = chunk.get(2).copied().unwrap_or(0) as usize;
        output.push(TABLE[first >> 2] as char);
        output.push(TABLE[((first & 3) << 4) | (second >> 4)] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((second & 15) << 2) | (third >> 6)] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[third & 63] as char
        } else {
            '='
        });
    }
    output
}

#[async_trait]
impl ExtensionHandler for PreviewHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        if method != "proxy" {
            return Err(ExtensionError::method_not_found());
        }
        if !matches!(self.runtime, PreviewRuntime::Web | PreviewRuntime::Desktop) {
            return Err(ExtensionError::capability_not_supported("preview.proxy"));
        }
        if ctx.session_id.is_none() || ctx.principal.trim().is_empty() {
            return Err(ExtensionError::forbidden(
                "preview proxy authorization required",
            ));
        }
        let request = Self::parse(params)?;
        Self::validate(&request)?;
        let url = format!("http://127.0.0.1:{}{}", request.port, request.path);
        let parsed_url = reqwest::Url::parse(&url).map_err(|_| ExtensionError::invalid_params("path is not allowed"))?;
        if parsed_url.host_str() != Some("127.0.0.1") || parsed_url.port_or_known_default() != Some(request.port) {
            return Err(Self::ssrf());
        }
        let mut builder = self
            .client
            .request(request.method.parse().map_err(|_| Self::internal())?, &url);
        for (name, value) in &request.headers {
            if name.eq_ignore_ascii_case("host")
                || name.eq_ignore_ascii_case("origin")
                || name.eq_ignore_ascii_case("content-length")
                || name.eq_ignore_ascii_case("connection")
                || name.eq_ignore_ascii_case("forwarded")
                || name.to_ascii_lowercase().starts_with("x-forwarded-")
            {
                continue;
            }
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        let response = builder.send().await.map_err(|error| {
            if error.is_timeout() {
                Self::timeout()
            } else if error.is_connect() {
                Self::unreachable()
            } else {
                Self::internal()
            }
        })?;
        let status = response.status().as_u16();
        let headers = Self::filtered_headers(&response);
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                if error.is_timeout() {
                    Self::timeout()
                } else {
                    Self::internal()
                }
            })?;
            if bytes.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(Self::internal());
            }
            bytes.extend_from_slice(&chunk);
        }
        let text =
            content_type.starts_with("text/") || content_type.starts_with("application/json");
        let (body, body_encoding) = if text {
            (
                String::from_utf8(bytes).map_err(|_| Self::internal())?,
                "utf-8".into(),
            )
        } else {
            (base64_encode(&bytes), "base64".into())
        };
        serde_json::to_value(PreviewProxyResponse {
            status,
            headers,
            body,
            body_encoding,
            from_cache: false,
        })
        .map_err(|_| Self::internal())
    }

    fn capabilities(&self) -> Value {
        if matches!(self.runtime, PreviewRuntime::Web | PreviewRuntime::Desktop) {
            serde_json::json!({"proxy": true})
        } else {
            serde_json::json!({})
        }
    }
}

impl Default for PreviewHandler {
    fn default() -> Self {
        Self::new()
    }
}
