# Serve Module

The **loom serve** command starts a WebSocket server for remote agent execution. Clients (e.g. the CLI with **--remote**, or IDEs) connect and send run requests; the server runs the agent and streams events back. This document covers server architecture, protocols, session management, and user messages.

## WebSocket server architecture

The serve module typically:

- Binds a WebSocket endpoint (e.g. host:port).
- Accepts connections and maintains a session per connection (or per run).
- Dispatches incoming messages as **ClientRequest** (Run, ToolsList, ToolShow, Ping).
- Runs the agent (e.g. **run_agent_with_options** or equivalent) for **RunRequest** and sends **ServerResponse** (RunStreamEvent, RunEnd, Error).
- Handles **ToolsListRequest** / **ToolShowRequest** by querying the tool source or run state; returns **ToolsListResponse** / **ToolShowResponse**.
- Responds to **PingRequest** with **PongResponse**.

Application state (**AppState** or similar) holds shared resources: optional store, user message store, and any config needed to build runners.

## Remote execution protocols

- **Client → Server**: JSON messages with a type and payload (e.g. **RunRequest** with message, thread_id, profile).
- **Server → Client**: **RunStreamEventResponse** (stream events), **RunEndResponse** (final state or error), **ToolsListResponse**, **ToolShowResponse**, **PongResponse**, **ErrorResponse**.
- Stream events use the same envelope format as **protocol::stream** (**stream_event_to_protocol_envelope** / **stream_event_to_protocol_format**) so the CLI and other clients can parse them uniformly.

## Session management

- Each WebSocket connection may be treated as a session. Thread identity is carried in **RunRequest** (thread_id, user_id) so multiple runs can share the same thread (e.g. resume after interrupt).
- The server does not necessarily persist sessions; checkpoint and store persistence are handled by the checkpointer and store (SQLite or in-memory) configured when building the runner.

## Tool listing and status

- **ToolsListRequest**: Server returns **ToolsListResponse** with tool specs (from the agent’s ToolSource, e.g. **list_tools()**). Used by clients to show available tools.
- **ToolShowRequest** / **ToolShowResponse**: Optional; show output or status for a specific tool call (e.g. by call_id or run id). Implementation may cache tool outputs from the last run or expose a minimal status.

## User message management

- **UserMessagesRequest** / **UserMessagesResponse**: Optional protocol for listing or appending user messages per thread. The **user_message** module provides **UserMessageStore** (e.g. **SqliteUserMessageStore**, **NoOpUserMessageStore**) for per-thread message history. When the server supports it, clients can fetch or append messages for a thread before or after a run.

## Summary

| Topic | Notes |
|-------|--------|
| Server | WebSocket endpoint; dispatch ClientRequest; run agent; send ServerResponse |
| Protocol | RunRequest → RunStreamEventResponse + RunEndResponse; ToolsList, ToolShow, Ping/Pong |
| Sessions | Thread/user in request; checkpoint and store provide persistence |
| Tools | ToolsListResponse from ToolSource; optional ToolShow for status/output |
| User messages | UserMessageStore; optional UserMessages request/response |

Next: [Advanced Patterns](../architecture/advanced-patterns.md) for DUP, GoT, ToT, and StateUpdater strategies.
