# ACP (Agent Client Protocol)

ACP (Agent Client Protocol) allows Loom to run as an agent that IDEs or other clients drive over stdio using JSON-RPC. This document summarizes the protocol and session lifecycle; for full details see the ACP spec and the **acp** (or protocol) implementation in the repo.

## JSON-RPC protocol details

- Communication is over **stdio**: client sends JSON-RPC requests to the process stdin; the agent responds on stdout.
- Requests and responses follow JSON-RPC 2.0 (id, method, params / result / error).
- The agent exposes methods for running the agent, listing tools, and possibly other lifecycle or capability methods.

## Client–server communication

- **Client** (IDE or CLI acting as client) spawns the Loom process and sends requests (e.g. "run" with message and options).
- **Server** (Loom process) parses requests, runs the graph or returns tool list, and streams or returns results via JSON-RPC responses/notifications.
- Streaming may be represented as a series of notifications or a result channel, depending on the ACP binding (e.g. progress, tool calls, final answer).

## Session lifecycle

- A **session** may correspond to one process run: connect (spawn) → send requests → receive responses → disconnect (process exit).
- Alternatively, long-lived processes may support multiple "sessions" or runs over the same stdio; the exact lifecycle is defined by the ACP implementation (e.g. init, run, shutdown).

## IDE integration patterns

- IDEs start the agent as a subprocess and communicate via stdio.
- Typical flow: initialize → optionally list tools → run with user message → consume streamed or final output.
- Loom's ACP integration (when present) maps **ReactRunner** (or other runner) invocations and **StreamEvent** to the protocol messages expected by the IDE.

## Session loading and replay

Loom supports loading existing sessions via `session/load` (when `capabilities.loadSession: true` is declared in `initialize`):

1. **Client sends `session/load`** with `session_id`, `working_directory`, and `mcp_servers`.
2. **Agent loads checkpoint** using `session_id` as `thread_id` from the checkpointer (SQLite by default at `~/.loom/memory.db`).
3. **Agent replays history** by sending `session/update` notifications:
   - `user_message_chunk` for each User message
   - `agent_message_chunk` for each Assistant message
   - System messages are skipped (not sent to client)
4. **Agent creates session entry** in SessionStore if it doesn't exist.
5. **Agent returns `LoadSessionResponse`** after history is sent.

The replayed messages allow the client to restore the conversation UI state. After loading, the session can continue with new `session/prompt` requests that will append to the existing history.

**Implementation note**: The `load_session_internal` method in `LoomAcpAgent` handles the loading logic. If the checkpoint doesn't exist, the session starts fresh without error.

## Summary

| Topic | Notes |
|-------|--------|
| Transport | stdio; JSON-RPC 2.0 |
| Methods | initialize, session/new, session/load, session/prompt, etc. (see ACP spec) |
| Session | Process lifecycle or multi-run over one process |
| Session loading | Load existing sessions from checkpointer and replay history via session/update |
| IDE | Spawn process; stdio; run and stream results |

Next: [Compression](../architecture/compression.md) for message pruning and compaction.
