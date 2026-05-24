pub mod event_handler;
pub mod format;
pub mod markdown;
pub mod panel_format;
pub mod spinner;
pub mod tool_preview;
pub mod tool_summary;

pub use event_handler::{create_stdio_event_callback, find_tool_result, find_tool_result_error, EventState, StreamDisplayConfig};
pub use markdown::render_markdown;
pub use panel_format::{
    dim, format_agent_line, format_model_line, format_panel_line, format_thinking_separator,
    format_tool_call, format_tool_done, format_tools_line, format_usage_line,
};
pub use spinner::{NoopSpinner, Spinner, SpinnerTrait};
pub use tool_preview::{format_diff, format_preview, format_result_preview};
pub use tool_summary::{format_call_summary, format_done_summary, format_elapsed, truncate};
