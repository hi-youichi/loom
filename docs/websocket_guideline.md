# WebSocket API Guideline

This document describes how to use the Loom WebSocket server: connection, request/response shapes, and usage patterns.

**Protocol scope**: This guideline defines all **client requests** and **server responses** (message types and fields). The **streaming event payload** inside `run_stream_event` (event types, envelope, reply shape) is defined in [protocol_spec.md](./protocol_spec.md). Together, this document and protocol_spec.md are the full WebSocket protocol.

---

## 1. Overview

- **Server**: Loom WebSocket server (serve crate), used by the `loom serve` CLI.
- **Default address**: `ws://127.0.0.1:8080`.
- **Endpoint**: Single route `GET /`; upgrade to WebSocket, then send/receive JSON text or binary frames (UTF-8).
- **Model**: Client sends one JSON **request** per message; server sends one or more JSON **responses** (for `run`, many stream events then one `run_end`).

**Run the server**

```bash
cargo run -p cli -- serve
# Or: loom serve

# Custom address
cargo run -p cli -- serve --addr 127.0.0.1:9000
```

---

## 2. Connection and Encoding

- **Encoding**: UTF-8.
- **Frame**: One complete JSON object per WebSocket **text** or **binary** message. Server treats binary as UTF-8 and parses as JSON. Other frame types (e.g. Ping, Pong, Close) are not treated as requests; the server only parses Text/Binary as client requests.
- **Order**: Responses are ordered; for a single `run` request, all `run_stream_event` frames are sent in order, then exactly one `run_end` or one `error`.
- **Connection lifecycle**: The server handles one request per message; after sending the response(s), it waits for the next message. On **parse error** it sends a single `error` and keeps the connection open. On **request handling error** (e.g. run failure), it sends `error` and then closes the connection.

---

## 3. Client Requests (client → server)

Every client message is a JSON object with a **`type`** field. Supported values:

| type             | Description                    |
|------------------|--------------------------------|
| `run`            | Execute one agent run (streaming + final reply). |
| `tools_list`     | List all tools.                |
| `tool_show`      | Get one tool definition.       |
| `user_messages`  | List stored messages for a thread (user message store). |
| `ping`           | Health / keepalive.            |

### 3.1 `run`

Execute one agent run. Server will send multiple `run_stream_event` messages (streaming events), then one `run_end` with the full reply, or one `error` on failure.

| Field             | Type   | Required | Description |
|-------------------|--------|----------|-------------|
| `type`            | string | Yes      | `"run"`     |
| `message`         | string | Yes      | User message to the agent. |
| `agent`           | string | Yes      | `"react"`, `"dup"`, `"tot"`, or `"got"`. |
| `id`              | string | No       | Optional request id. |
| `thread_id`       | string | No       | Session/thread id (e.g. for memory/checkpoints). |
| `working_folder`  | string | No       | Working directory path (tool resolution, prompts). |
| `got_adaptive`    | bool   | No       | For `agent: "got"` only; enable AGoT adaptive mode. |
| `verbose`         | bool   | No       | Enable verbose logging. |

**Role (SOUL)**: The server does not accept a role file in the request. The agent’s role/persona is resolved at runtime from `SOUL.md` under the request’s `working_folder` (if present), otherwise from the built-in default. To override role per run from the client, use a `working_folder` that contains the desired `SOUL.md`. The CLI supports an explicit `--role FILE`; the WebSocket API does not.

**Example**

```json
{
  "type": "run",
  "message": "What time is it?",
  "agent": "react"
}
```

### 3.2 `tools_list`

List all tools (optionally scoped by `working_folder` and `thread_id`).

| Field             | Type   | Required | Description |
|-------------------|--------|----------|-------------|
| `type`            | string | Yes      | `"tools_list"` |
| `id`              | string | Yes      | Request id (echoed in response). |
| `working_folder`  | string | No       | Working directory. |
| `thread_id`       | string | No       | Thread id. |

### 3.3 `tool_show`

Get a single tool definition (JSON or YAML).

| Field             | Type   | Required | Description |
|-------------------|--------|----------|-------------|
| `type`            | string | Yes      | `"tool_show"` |
| `id`              | string | Yes      | Request id. |
| `name`            | string | Yes      | Tool name (e.g. `"read"`, `"web_fetcher"`). |
| `output`          | string | No       | `"yaml"` (default) or `"json"`. |
| `working_folder`  | string | No       | Working directory. |
| `thread_id`       | string | No       | Thread id. |

### 3.5 `user_messages`

List stored messages for a thread. Used when the server is configured with a user message store (e.g. `USER_MESSAGE_DB`). When no store is configured, the server returns `messages: []` and `has_more: false` (no error).

| Field       | Type   | Required | Description |
|-------------|--------|----------|-------------|
| `type`      | string | Yes      | `"user_messages"` |
| `id`        | string | Yes      | Request id (echoed in response). |
| `thread_id` | string | Yes      | Thread id; required. Missing or empty returns `error`. |
| `before`    | number | No       | Pagination cursor (message seq/id); return only messages with seq &lt; `before`. |
| `limit`     | number | No       | Max number of messages to return (default 100, capped). |

### 3.6 `ping`

Health / keepalive. Server responds with `pong` and the same `id`.

| Field   | Type   | Required | Description |
|---------|--------|----------|-------------|
| `type`  | string | Yes      | `"ping"`    |
| `id`    | string | Yes      | Echoed in `pong`. |

---

## 4. Server Responses (server → client)

Every server message is a JSON object with a **`type`** field. Supported values:

| type               | Description |
|--------------------|-------------|
| `run_stream_event` | One streaming event for a run (see [protocol_spec.md](./protocol_spec.md) for event payload). |
| `run_end`          | Final reply for one run; sent after all stream events for that run. |
| `tools_list`       | Response to `tools_list`. |
| `tool_show`        | Response to `tool_show`. |
| `user_messages`    | Response to `user_messages`; list of messages for the thread. |
| `pong`             | Response to `ping`. |
| `error`            | Request failed (parse error, run error, etc.). |

### 4.1 `run_stream_event`

Payload:

| Field   | Type   | Description |
|---------|--------|-------------|
| `type`  | string | `"run_stream_event"` |
| `id`    | string | Run id (same for all events and the final `run_end` for this run). |
| `event` | object | One streaming event; shape follows [protocol_spec.md](./protocol_spec.md) (e.g. `type`: `run_start`, `node_enter`, `message_chunk`, `usage`, `node_exit`, tool events, etc.). May include `session_id`, `node_id`, `event_id`. |

### 4.2 `run_end`

Payload:

| Field        | Type   | Description |
|--------------|--------|-------------|
| `type`       | string | `"run_end"` |
| `id`         | string | Run id (matches `run_stream_event.id`). |
| `reply`      | string | Full assistant reply for this run. |
| `usage`      | object | Optional; token usage for last call. |
| `total_usage`| object | Optional; cumulative usage for the run. |
| `session_id` | string | Optional; aligns with stream events. |
| `node_id`    | string | Optional. |
| `event_id`   | number | Optional. |

**Usage object** (for `usage` and `total_usage` in `run_end`):

| Field               | Type   | Description |
|---------------------|--------|-------------|
| `prompt_tokens`     | number | Input tokens. |
| `completion_tokens` | number | Output tokens. |
| `total_tokens`      | number | prompt_tokens + completion_tokens. |

### 4.3 `tools_list`

Payload:

| Field   | Type  | Description |
|---------|-------|-------------|
| `type`  | string | `"tools_list"` |
| `id`    | string | Request id from client. |
| `tools` | array | List of tool specs; each element is a **ToolSpec** object (see below). |

**ToolSpec** (each element of `tools`):

| Field           | Type   | Description |
|-----------------|--------|-------------|
| `name`          | string | Tool name (e.g. `"read"`, `"bash"`). |
| `description`   | string | Optional; human-readable description for the LLM. |
| `input_schema`  | object | JSON Schema for the tool's arguments (e.g. `type`, `properties`, `required`). |

### 4.4 `tool_show`

Payload:

| Field       | Type   | Description |
|-------------|--------|-------------|
| `type`      | string | `"tool_show"` |
| `id`        | string | Request id. |
| `tool`      | object | Present when client sent `output: "json"`; full tool definition as JSON. |
| `tool_yaml` | string | Present when client sent `output: "yaml"` or omitted; tool definition as YAML string. |

Exactly one of `tool` or `tool_yaml` is present.

### 4.5 `user_messages`

Payload:

| Field       | Type   | Description |
|-------------|--------|--------------|
| `type`      | string | `"user_messages"` |
| `id`        | string | Request id from client. |
| `thread_id` | string | Thread id. |
| `messages`  | array  | List of message objects in order (oldest first). |
| `has_more`  | bool   | Optional; `true` if more messages exist (pagination). |

Each element of `messages`:

| Field     | Type   | Description |
|-----------|--------|-------------|
| `role`    | string | `"system"`, `"user"`, or `"assistant"`. |
| `content` | string | Message content. |

When no user message store is configured, the server returns `messages: []` and `has_more: false`.

### 4.6 `pong`

| Field  | Type   | Description |
|--------|--------|-------------|
| `type` | string | `"pong"` |
| `id`   | string | Same as in `ping`. |

### 4.7 `error`

| Field   | Type   | Description |
|---------|--------|-------------|
| `type`  | string | `"error"` |
| `id`    | string | Optional; run id if error occurred during a run. |
| `error` | string | Error message. |

---

## 5. Run Flow (single run)

1. Client sends one **run** request.
2. Server sends zero or more **run_stream_event** messages (ordered; event payloads follow protocol_spec).
3. Server sends exactly one **run_end** (success) or one **error** (failure) for that run.
4. Client can correlate by the **id** in `run_stream_event` and `run_end`/`error`.

No request id is required for `run`; the server assigns a run `id` and returns it in every `run_stream_event` and in `run_end`.

---

## 6. Best Practices

- **Ordering**: Process responses in the order received; stream events and run_end are ordered per run.
- **Errors**: On parse failure, server sends a single `error` and does not close the connection. On run failure, server sends `error` with optional `id` and may close the connection depending on implementation.
- **Optional fields**: Omit optional request fields or set them to `null`; server uses defaults. Response optional fields may be absent.
- **Streaming events**: The inner `event` in `run_stream_event` has a `type` (e.g. `run_start`, `node_enter`, `message_chunk`, `node_exit`, tool_*). See [protocol_spec.md](./protocol_spec.md) for full event types and payloads.
- **One run at a time**: The server handles one request at a time per connection; wait for `run_end` or `error` before sending another `run` if you need strict ordering.

---

## 7. Summary of types

| Role   | type / variant   | Rust type / inner |
|--------|------------------|-------------------|
| Client | `run`            | `RunRequest` (message, agent, id?, thread_id?, working_folder?, got_adaptive?, verbose?) |
| Client | `tools_list`     | `ToolsListRequest` (id, working_folder?, thread_id?) |
| Client | `tool_show`      | `ToolShowRequest` (id, name, output?, working_folder?, thread_id?) |
| Client | `user_messages`  | `UserMessagesRequest` (id, thread_id, before?, limit?) |
| Client | `ping`           | `PingRequest` (id) |
| Server | `run_stream_event` | `RunStreamEventResponse` (id, event: ProtocolEventEnvelope) |
| Server | `run_end`        | `RunEndResponse` (id, reply, usage?, total_usage?, session_id?, node_id?, event_id?) |
| Server | `tools_list`     | `ToolsListResponse` (id, tools: Vec&lt;ToolSpec&gt;) |
| Server | `tool_show`      | `ToolShowResponse` (id, tool or tool_yaml) |
| Server | `user_messages`  | `UserMessagesResponse` (id, thread_id, messages: Vec&lt;UserMessageItem&gt;, has_more?) |
| Server | `pong`           | `PongResponse` (id) |
| Server | `error`          | `ErrorResponse` (id?, error) |

## 8. References

- [protocol_spec.md](./protocol_spec.md) — Streaming event types and envelope (session_id, node_id, event_id), reply message shape.
- **Rust types**: `loom::protocol` — `ClientRequest`, `ServerResponse`, `RunRequest`, `AgentType`, `RunStreamEventResponse`, `RunEndResponse`, `UserMessagesRequest`, `UserMessagesResponse`, `UserMessageItem`, `ToolSpec`, `LlmUsage`, etc.
