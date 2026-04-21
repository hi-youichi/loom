#[allow(unused_imports)]
pub mod test_setup;
#[allow(unused_imports)]
pub mod config_helpers;
pub mod acp_child;
#[allow(dead_code)]
pub mod plan_types;

#[allow(unused_imports)]
pub use test_setup::TestEnvironment;
#[allow(unused_imports)]
pub use config_helpers::{create_agent_config, write_last_model_file};

#[derive(Debug, serde::Deserialize)]
pub struct RpcResponse {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub id: Option<serde_json::Value>,
    #[allow(dead_code)]
    pub result: Option<serde_json::Value>,
    #[allow(dead_code)]
    pub error: Option<RpcError>,
}

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

#[allow(unused_imports)]
pub use acp_child::{AcpChild, ToolCallResponse, MockAcpServer};
#[allow(unused_imports)]
pub use plan_types::{PlanNotification, PlanEntry, PlanEntryPriority, PlanEntryStatus};
