//! Pagination helpers for list-style extension methods.
//!
//! Cursor is a JSON object encoded as hex string to remain opaque to clients.

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::ExtensionError;

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, ()> {
    if !hex.len().is_multiple_of(2) {
        return Err(());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| ()))
        .collect()
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PaginationParams {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

impl PaginationParams {
    pub fn limit_or_default(&self, default: usize, max: usize) -> usize {
        self.limit.unwrap_or(default).min(max)
    }

    pub fn decode_cursor<T: DeserializeOwned>(&self) -> Result<Option<T>, ExtensionError> {
        match &self.cursor {
            None => Ok(None),
            Some(raw) => {
                let bytes = hex_decode(raw)
                    .map_err(|_| ExtensionError::invalid_params("invalid cursor encoding"))?;
                let value: T = serde_json::from_slice(&bytes)
                    .map_err(|e| ExtensionError::invalid_params(format!("invalid cursor: {e}")))?;
                Ok(Some(value))
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PaginatedResult<T: Serialize> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

impl<T: Serialize + Clone> PaginatedResult<T> {
    pub fn new(items: Vec<T>, next_cursor: Option<String>) -> Self {
        let has_more = next_cursor.is_some();
        Self {
            items,
            next_cursor,
            has_more,
        }
    }

    pub fn empty() -> Self {
        Self {
            items: vec![],
            next_cursor: None,
            has_more: false,
        }
    }

    pub fn from_slice(full: Vec<T>, offset: usize, limit: usize) -> Self {
        let total = full.len();
        let end = (offset + limit).min(total);
        let items = full[offset..end].to_vec();
        let has_more = end < total;
        let next_cursor = if has_more {
            Some(encode_cursor(serde_json::json!({ "offset": end })))
        } else {
            None
        };
        Self {
            items,
            next_cursor,
            has_more,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "items": self.items,
            "nextCursor": self.next_cursor,
            "hasMore": self.has_more,
        })
    }
}

pub fn encode_cursor(value: serde_json::Value) -> String {
    let json = serde_json::to_string(&value).unwrap_or_default();
    hex_encode(json.as_bytes())
}
