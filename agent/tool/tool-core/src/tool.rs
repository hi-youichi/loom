use async_trait::async_trait;
use serde_json::Value;

use crate::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;

    fn spec(&self) -> ToolSpec;

    async fn call(
        &self,
        args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError>;
}
