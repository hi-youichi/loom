//! Serializer for checkpoint state (state <-> bytes).
//!
//! Serializer protocol. Used by persistent
//! Checkpointer implementations.
//!
//! ## Protocol Overview
//!
//! This module provides two serialization protocols:
//!
//! 1. **Serializer<S>** - Simple serialize/deserialize for typed state
//! 2. **TypedSerializer** - Typed serialization with type tag (matches Python's SerializerProtocol)
//!
//! The typed serialization uses a `(type, bytes)` tuple where `type` indicates the encoding:
//! - `"null"` - None/empty value
//! - `"bytes"` - Raw bytes (no transformation)
//! - `"json"` - JSON-encoded data

use crate::memory::checkpointer::CheckpointError;

/// Type tag for null/empty values.
pub const TYPE_NULL: &str = "null";
/// Type tag for raw bytes.
pub const TYPE_BYTES: &str = "bytes";
/// Type tag for JSON-encoded data.
pub const TYPE_JSON: &str = "json";

/// Typed serialization data - tuple of (type_tag, bytes).
///
/// Aligns with Python's `SerializerProtocol.dumps_typed` return type.
#[derive(Debug, Clone)]
pub struct TypedData {
    /// Type tag indicating the encoding (null, bytes, json).
    pub type_tag: String,
    /// Serialized bytes (empty for null type).
    pub data: Vec<u8>,
}

impl TypedData {
    /// Creates a null typed data (empty).
    pub fn null() -> Self {
        Self {
            type_tag: TYPE_NULL.to_string(),
            data: Vec::new(),
        }
    }

    /// Creates a bytes typed data (raw bytes, no encoding).
    pub fn bytes(data: Vec<u8>) -> Self {
        Self {
            type_tag: TYPE_BYTES.to_string(),
            data,
        }
    }

    /// Creates a JSON typed data.
    pub fn json(data: Vec<u8>) -> Self {
        Self {
            type_tag: TYPE_JSON.to_string(),
            data,
        }
    }

    /// Returns true if this is a null value.
    pub fn is_null(&self) -> bool {
        self.type_tag == TYPE_NULL
    }
}

/// Serializes and deserializes state for checkpoint storage.
///
/// Used by persistent Checkpointer implementations (e.g. SqliteSaver). MemorySaver
/// stores `Checkpoint<S>` in memory and does not use a Serializer.
///
/// **Interaction**: Used by SqliteSaver for persisting state to SQLite.
pub trait Serializer<S>: Send + Sync
where
    S: Clone + Send + Sync + 'static,
{
    /// Serialize state to bytes.
    fn serialize(&self, state: &S) -> Result<Vec<u8>, CheckpointError>;

    /// Deserialize state from bytes.
    fn deserialize(&self, bytes: &[u8]) -> Result<S, CheckpointError>;
}

/// Typed serialization protocol - aligns with Python's SerializerProtocol.
///
/// Provides type-tagged serialization where the type tag indicates the encoding.
/// This enables interoperability and allows storing different data types.
///
/// **Interaction**: Used by TypedJsonSerializer for advanced checkpoint storage.
pub trait TypedSerializer: Send + Sync {
    /// Serialize any value to typed data (type_tag, bytes).
    ///
    /// Returns a TypedData with type tag indicating the encoding.
    fn dumps_typed(&self, value: &serde_json::Value) -> Result<TypedData, CheckpointError>;

    /// Deserialize typed data (type_tag, bytes) back to a value.
    fn loads_typed(&self, data: &TypedData) -> Result<serde_json::Value, CheckpointError>;
}

/// JSON-based serializer. Requires S: Serialize + serde::de::DeserializeOwned.
///
/// Use for persistent checkpoint storage when state is JSON-serializable.
///
/// **Interaction**: Injected into SqliteSaver for state serialization.
pub struct JsonSerializer;

impl<S> Serializer<S> for JsonSerializer
where
    S: Clone + Send + Sync + 'static + serde::Serialize + serde::de::DeserializeOwned,
{
    fn serialize(&self, state: &S) -> Result<Vec<u8>, CheckpointError> {
        serde_json::to_vec(state).map_err(|e| CheckpointError::Serialization(e.to_string()))
    }

    fn deserialize(&self, bytes: &[u8]) -> Result<S, CheckpointError> {
        serde_json::from_slice(bytes).map_err(|e| CheckpointError::Serialization(e.to_string()))
    }
}

impl TypedSerializer for JsonSerializer {
    fn dumps_typed(&self, value: &serde_json::Value) -> Result<TypedData, CheckpointError> {
        if value.is_null() {
            return Ok(TypedData::null());
        }
        let bytes =
            serde_json::to_vec(value).map_err(|e| CheckpointError::Serialization(e.to_string()))?;
        Ok(TypedData::json(bytes))
    }

    fn loads_typed(&self, data: &TypedData) -> Result<serde_json::Value, CheckpointError> {
        match data.type_tag.as_str() {
            TYPE_NULL => Ok(serde_json::Value::Null),
            TYPE_BYTES => {
                // Return bytes as a JSON string (base64-encoded would be better for binary)
                Ok(serde_json::Value::String(
                    String::from_utf8_lossy(&data.data).to_string(),
                ))
            }
            TYPE_JSON => serde_json::from_slice(&data.data)
                .map_err(|e| CheckpointError::Serialization(e.to_string())),
            other => Err(CheckpointError::Serialization(format!(
                "Unknown serialization type: {}",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct TestState {
        value: String,
    }

    /// **Scenario**: Serialize then deserialize yields the same value.
    #[test]
    fn json_serializer_roundtrip() {
        let ser = JsonSerializer;
        let state = TestState {
            value: "hello".into(),
        };
        let bytes = ser.serialize(&state).unwrap();
        let restored: TestState = ser.deserialize(&bytes).unwrap();
        assert_eq!(state, restored);
    }

    /// **Scenario**: Invalid JSON on deserialize returns CheckpointError::Serialization.
    #[test]
    fn json_serializer_invalid_json_deserialize_returns_checkpoint_error() {
        let ser = JsonSerializer;
        let invalid = b"{ not valid json ]";
        let result: Result<TestState, _> = ser.deserialize(invalid);
        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            CheckpointError::Serialization(s) => assert!(!s.is_empty()),
            _ => panic!("expected Serialization variant: {:?}", err),
        }
    }

    /// **Scenario**: TypedData::null creates null type.
    #[test]
    fn typed_data_null() {
        let data = TypedData::null();
        assert_eq!(data.type_tag, TYPE_NULL);
        assert!(data.data.is_empty());
        assert!(data.is_null());
    }

    /// **Scenario**: TypedData::bytes creates bytes type.
    #[test]
    fn typed_data_bytes() {
        let raw = vec![1, 2, 3, 4];
        let data = TypedData::bytes(raw.clone());
        assert_eq!(data.type_tag, TYPE_BYTES);
        assert_eq!(data.data, raw);
        assert!(!data.is_null());
    }

    /// **Scenario**: TypedData::json creates json type.
    #[test]
    fn typed_data_json() {
        let raw = b"{\"x\": 1}".to_vec();
        let data = TypedData::json(raw.clone());
        assert_eq!(data.type_tag, TYPE_JSON);
        assert_eq!(data.data, raw);
    }

    /// **Scenario**: dumps_typed with null value returns null type.
    #[test]
    fn typed_serializer_dumps_null() {
        let ser = JsonSerializer;
        let data = ser.dumps_typed(&serde_json::Value::Null).unwrap();
        assert_eq!(data.type_tag, TYPE_NULL);
        assert!(data.data.is_empty());
    }

    /// **Scenario**: dumps_typed with object returns json type.
    #[test]
    fn typed_serializer_dumps_json() {
        let ser = JsonSerializer;
        let value = json!({"name": "test", "count": 42});
        let data = ser.dumps_typed(&value).unwrap();
        assert_eq!(data.type_tag, TYPE_JSON);
        assert!(!data.data.is_empty());
    }

    /// **Scenario**: loads_typed with null type returns Null.
    #[test]
    fn typed_serializer_loads_null() {
        let ser = JsonSerializer;
        let data = TypedData::null();
        let value = ser.loads_typed(&data).unwrap();
        assert!(value.is_null());
    }

    /// **Scenario**: loads_typed with json type returns deserialized value.
    #[test]
    fn typed_serializer_loads_json() {
        let ser = JsonSerializer;
        let original = json!({"items": [1, 2, 3]});
        let data = ser.dumps_typed(&original).unwrap();
        let restored = ser.loads_typed(&data).unwrap();
        assert_eq!(original, restored);
    }

    /// **Scenario**: loads_typed with unknown type returns error.
    #[test]
    fn typed_serializer_loads_unknown_type() {
        let ser = JsonSerializer;
        let data = TypedData {
            type_tag: "unknown".to_string(),
            data: vec![],
        };
        let result = ser.loads_typed(&data);
        assert!(result.is_err());
        match result.unwrap_err() {
            CheckpointError::Serialization(msg) => {
                assert!(msg.contains("Unknown serialization type"));
            }
            _ => panic!("expected Serialization error"),
        }
    }

    /// **Scenario**: Roundtrip typed serialization preserves complex data.
    #[test]
    fn typed_serializer_roundtrip_complex() {
        let ser = JsonSerializer;
        let original = json!({
            "nested": {
                "array": [1, "two", null, true],
                "number": 3.14
            },
            "string": "hello world"
        });
        let data = ser.dumps_typed(&original).unwrap();
        let restored = ser.loads_typed(&data).unwrap();
        assert_eq!(original, restored);
    }
}
