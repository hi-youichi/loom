//! Date tool: returns the current date and time.
//!
//! Supports optional custom format (strftime) and timezone offset.
//! Uses `chrono` for datetime operations.

use async_trait::async_trait;
use chrono::{FixedOffset, Local, TimeZone, Utc};
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::Tool;

/// Tool name constant.
pub const TOOL_DATE: &str = "date";

/// Default format string (ISO 8601 with timezone offset).
const DEFAULT_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%:z";

pub struct DateTool;

impl DateTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DateTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for DateTool {
    fn name(&self) -> &str {
        TOOL_DATE
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_DATE.to_string(),
            description: Some(
                "Returns the current date and time. By default returns the local datetime in ISO 8601 format. \
                 Supports optional timezone offset (e.g., \"+09:00\", \"-05:00\", \"UTC\") \
                 and custom format strings (e.g., \"%Y-%m-%d\", \"%H:%M:%S\"). \
                 Useful when the agent needs to know the current date/time for scheduling, timestamps, \
                 date arithmetic, or any time-sensitive operations."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "description": "Optional strftime format string. Default is ISO 8601 \"%Y-%m-%dT%H:%M:%S%:z\". Examples: \"%Y-%m-%d\" for date only, \"%H:%M:%S\" for time only."
                    },
                    "timezone": {
                        "type": "string",
                        "description": "Optional timezone offset or name. Examples: \"UTC\", \"+09:00\", \"-05:00\". Default is the system local timezone."
                    }
                }
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let fmt = args
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_FORMAT);

        let tz_str = args.get("timezone").and_then(|v| v.as_str());

        let formatted = if let Some(tz) = tz_str {
            if tz.eq_ignore_ascii_case("UTC") || tz == "Z" {
                Utc::now().format(fmt).to_string()
            } else {
                // Try parsing as fixed offset like "+09:00" or "-05:00"
                let offset: FixedOffset = tz.parse().map_err(|e| {
                    ToolSourceError::InvalidInput(format!(
                        "invalid timezone '{}': {}. Use \"UTC\" or offset like \"+09:00\", \"-05:00\".",
                        tz, e
                    ))
                })?;
                offset.from_utc_datetime(&Utc::now().naive_utc()).format(fmt).to_string()
            }
        } else {
            Local::now().format(fmt).to_string()
        };

        Ok(ToolCallContent::text(formatted))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn date_tool_name() {
        let tool = DateTool::new();
        assert_eq!(tool.name(), TOOL_DATE);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn date_tool_spec_has_name() {
        let tool = DateTool::new();
        let spec = tool.spec();
        assert_eq!(spec.name, TOOL_DATE);
        assert!(spec.description.is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn date_tool_default_returns_iso8601() {
        let tool = DateTool::new();
        let result = tool.call(json!({}), None).await.unwrap();
        let text = result.as_text().unwrap();
        // Should look like "2026-05-26T21:30:00+09:00"
        assert!(text.contains('T'), "expected ISO 8601 format, got: {}", text);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn date_tool_custom_format() {
        let tool = DateTool::new();
        let result = tool.call(json!({ "format": "%Y-%m-%d" }), None).await.unwrap();
        let text = result.as_text().unwrap();
        // Should be like "2026-05-26" (no 'T')
        assert!(!text.contains('T'), "expected date only, got: {}", text);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn date_tool_utc_timezone() {
        let tool = DateTool::new();
        let result = tool
            .call(json!({ "timezone": "UTC", "format": "%Y-%m-%dT%H:%M:%S%:z" }), None)
            .await
            .unwrap();
        let text = result.as_text().unwrap();
        assert!(text.ends_with("+00:00"), "expected UTC offset, got: {}", text);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn date_tool_invalid_timezone() {
        let tool = DateTool::new();
        let result = tool.call(json!({ "timezone": "invalid" }), None).await;
        assert!(result.is_err(), "expected error for invalid timezone");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn date_tool_offset_timezone() {
        let tool = DateTool::new();
        let result = tool
            .call(json!({ "timezone": "+09:00", "format": "%Y-%m-%dT%H:%M:%S%:z" }), None)
            .await
            .unwrap();
        let text = result.as_text().unwrap();
        assert!(text.contains('+'), "expected offset in output, got: {}", text);
    }
}
