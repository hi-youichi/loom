pub mod test_setup;
pub mod config_helpers;
pub mod acp_child;

// Only export specific items that are actually used
pub use test_setup::TestEnvironment;
#[allow(unused_imports)]
pub use config_helpers::{create_agent_config, write_last_model_file};

// Common types for RPC communication
#[derive(Debug, serde::Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: Option<String>,
    pub id: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
    pub error: Option<RpcError>,
}

#[derive(Debug, serde::Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

// Export AcpChild and related types that are used in e2e tests
pub use acp_child::{AcpChild, ToolCallResponse};