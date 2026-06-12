//! Web tools registration: web_fetcher for HTTP GET/POST requests.

use tool_core::{ToolRegistryLocked};
use crate::WebFetcherTool;

pub use loom_types::tools::tool_name::TOOL_WEB_FETCHER;

/// Register web fetcher tools with the given registry.
///
/// This function registers the [`WebFetcherTool`] with the provided tool registry.
///
/// # Examples
///
/// ```no_run
/// use tool_basic::register_web_tools;
/// use tool_core::ToolRegistryLocked;
/// # #[tokio::main]
/// # async fn main() {
/// let registry = ToolRegistryLocked::new();
/// register_web_tools(&registry).await;
/// # }
/// ```
pub async fn register_web_tools(registry: &ToolRegistryLocked) {
    registry.register_async(Box::new(WebFetcherTool::new())).await;
}

/// Register web fetcher tools with a custom client.
///
/// This function registers the [`WebFetcherTool`] with a custom HTTP client.
///
/// # Examples
///
/// ```no_run
/// use tool_basic::register_web_tools_with_client;
/// use tool_core::ToolRegistryLocked;
/// # #[tokio::main]
/// # async fn main() {
/// let client = reqwest::Client::new();
/// let registry = ToolRegistryLocked::new();
/// register_web_tools_with_client(&registry, client).await;
/// # }
/// ```
pub async fn register_web_tools_with_client(registry: &ToolRegistryLocked, client: reqwest::Client) {
    registry.register_async(Box::new(WebFetcherTool::with_client(client))).await;
}