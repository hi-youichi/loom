//! # Protocol module
//!
//! - **WebSocket**: CLI remote mode request/response types. Aligned with [DESIGN_CLI_REMOTE_MODE]
//!   §2.3 (requests) and §2.4 (responses), and with [EXPORT_SPEC] / [USER_GUIDELINE].
//! - **Stream**: Streaming output protocol (type + payload, envelope) per [protocol_spec].
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                           protocol (this crate)                              │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                              │
//! │   Request types (client → server)          Response types (server → client)  │
//! │   ─────────────────────────────           ───────────────────────────────   │
//! │   ClientRequest:                           ServerResponse:                   │
//! │     Run(RunRequest)                          RunStreamEvent(RunStreamEventResponse)  │
//! │     ToolsList(ToolsListRequest)              RunEnd(RunEndResponse)          │
//! │     ToolShow(ToolShowRequest)                ToolsList(ToolsListResponse)     │
//! │     UserMessages(UserMessagesRequest)        UserMessages(UserMessagesResponse)  │
//! │     AgentList(AgentListRequest)              AgentList(AgentListResponse)     │
//! │     Ping(PingRequest)                        ToolShow(ToolShowResponse)       │
//! │                                              Pong(PongResponse)              │
//! │                                              Error(ErrorResponse)             │
//! │                                                                              │
//! │   ┌──────────────┐    JSON (type + payload)    ┌──────────────┐             │
//! │   │    Client    │ ─────────────────────────►  │    Server    │             │
//! │   │  (WebSocket) │  ◄───────────────────────── │  (WebSocket) │             │
//! │   └──────────────┘                              └──────┬───────┘             │
//! │        │                                                │                    │
//! │        ▼                                                ▼                    │
//! │   ┌─────────────────────────────────────────────────────────────────────┐  │
//! │   │  envelope_state    stream (stream_event bridge)                       │  │
//! │   │  EnvelopeState     StreamEvent<S> ──► ProtocolEvent ──► JSON envelope │  │
//! │   │                    ProtocolEventEnvelope, RunStreamEventResponse     │  │
//! │   └─────────────────────────────────────────────────────────────────────┘  │
//! │                                                                              │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! [DESIGN_CLI_REMOTE_MODE]: https://github.com/loom/loom/blob/main/docs/DESIGN_CLI_REMOTE_MODE.md
//! [EXPORT_SPEC]: https://github.com/loom/loom/blob/main/docs/EXPORT_SPEC.md
//! [USER_GUIDELINE]: https://github.com/loom/loom/blob/main/docs/USER_GUIDELINE.md
//! [protocol_spec]: https://github.com/loom/loom/blob/main/docs/protocol_spec.md

pub mod envelope_state;
pub mod stream;
pub mod requests;
pub mod responses;
pub mod types;

// Re-export sub-module types for convenience
pub use envelope_state::EnvelopeState;
pub use stream_event::ProtocolEvent;

// Re-export types from sub-modules
pub use requests::{
    AgentIdentifier,
    AgentListRequest, AgentSourceFilter, AgentType, ClientRequest, PingRequest, RunRequest,
    ToolShowOutput, ToolShowRequest, ToolsListRequest, UserMessagesRequest,
    WorkspaceListRequest, WorkspaceCreateRequest, WorkspaceThreadListRequest,
    WorkspaceThreadAddRequest, WorkspaceThreadRemoveRequest,
    ListModelsRequest, SetModelRequest,
};
pub use responses::{
    AgentListResponse, AgentSource, AgentSummary, ErrorResponse, PongResponse,
    ProtocolEventEnvelope, RunEndResponse, RunStreamEventResponse, ServerResponse,
    ToolShowResponse, ToolsListResponse, UserMessageItem, UserMessagesResponse,
    WorkspaceListResponse, WorkspaceMeta, WorkspaceCreateResponse,
    WorkspaceThreadListResponse, ThreadInWorkspace, WorkspaceThreadAddResponse,
    WorkspaceThreadRemoveResponse, ListModelsResponse, SetModelResponse,
};
pub use types::{AgentSource as AgentSourceExport, AgentSourceFilter as AgentSourceFilterExport};
