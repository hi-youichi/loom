pub mod event_handler;
pub mod format;
pub mod markdown;
pub mod panel_format;
pub mod spinner;
pub mod terminal;
pub mod streaming_markdown;
pub mod tool_preview;
pub mod tool_summary;

pub use markdown::render_markdown;
pub use terminal::{
    bold, dim, get_terminal_width, green, is_stderr_tty, is_stdout_tty,
    stderr_color_enabled, stdout_color_enabled, yellow,
};
pub use panel_format::{
   format_agent_line, format_model_line, format_panel_line, format_thinking_separator,
   format_tools_line, format_usage_line,
};
pub use format::format_context_limit;
pub use spinner::{NoopSpinner, Spinner, SpinnerTrait};
pub use streaming_markdown::StreamingMarkdownRenderer;
pub use tool_preview::{format_diff, format_preview, format_result_preview};
pub use tool_summary::{format_call_summary, format_elapsed, truncate};
pub use event_handler::{
    create_stdio_event_callback, find_tool_result, find_tool_result_error,
    EventState, StreamDisplayConfig,
    on_event_react, on_event_dup, on_event_tot, on_event_got,
    print_reply_timestamp, log_node_enter, print_stream_chunk,
};

pub use loom_stream::{MessageChunk, MessageChunkKind, StreamEvent};
pub use agent::state::{ReActState, ToolCall, ToolResult};
