# Streaming Output Protocol Specification (Protocol Spec)

This specification defines the **streaming output** data format and semantics for Loom Agent runs. Senders (server/CLI) and receivers (client/frontend) **must** produce and parse data according to this protocol so that multi-session, multi-turn, and per-node execution can be correctly merged and rendered.

**Transport form**: This spec defines the **JSON shape of each message (frame)**; it does not mandate the underlying transport (e.g. stdout NDJSON, WebSocket text frames, HTTP chunked). Each line or frame must be a single, complete JSON object.

---

## 1. Scope and Terminology

### 1.1 Scope

- **In scope**: Streaming events during an Agent run and the final reply for that run.
- **Out of scope**: Handshake, authentication, connection management, and non-streaming request/response formats (covered by other docs or implementation).

### 1.2 Terminology

| Term | Meaning |
|------|---------|
| **Session** | A continuous conversation, typically identified by a thread_id or connection. |
| **Node run** | Execution from node enter (node_enter) to node exit (node_exit); all events in that span share the same node_id. |
| **Event** | A single structured message in the stream, including types such as node_enter, node_exit, message_chunk, usage, values, updates (see §4). |
| **Reply** | The full assistant reply for the current run; the last message in the stream that corresponds to that run. |

---

## 2. Identifiers (Envelope Fields)

Each message (event line or reply line) JSON object **may** carry the following envelope fields at the top level; implementations are **encouraged** to include them for reliable merging across sessions and turns.

| Field | Type | Required | Meaning and constraints |
|-------|------|----------|-------------------------|
| **session_id** | string | No | Session ID. Constant within a session. |
| **node_id** | string | No | Node run ID. From node_enter to node_exit is one span; all lines in that span share the same node_id. **node_id may repeat** (across spans or runs); receivers use event_id or line order to distinguish. |
| **event_id** | number | No | Per-message sequence number. Monotonically increasing within a stream; unique per message; used for ordering, deduplication, and reference. |

Inside the event body, **id** denotes the node name (e.g. "think", "act"), and is distinct from the envelope **node_id**; the envelope node_id may repeat, while the body id is the node type name.

---

## 3. Transport and Encoding

- **Encoding**: UTF-8.
- **Frame boundary**: One complete JSON object per frame. For line-based transport (e.g. NDJSON), each line ends with `\n`; lines must not contain unescaped newlines.
- **Order**: Frame order matches event order; event_id (if present) increases monotonically.

---

## 4. Message Structure: Events

### 4.1 General Rules

- Event messages **must** include **type** (string) at the top level, indicating the event kind.
- Aside from type, remaining fields are payload; they sit at the same level as envelope fields (session_id, node_id, event_id).
- **Events** include run_start, node_enter, node_exit, message_chunk, usage, values, updates, custom, checkpoint, ToT/GoT-related types, and tool-related types (tool_call_chunk, tool_call, tool_start, tool_output, tool_end, tool_approval) (see §4.2).

### 4.2 Event Types and Payloads

The table below lists all event types and their payload fields (excluding type). `state` denotes the graph state JSON (shape depends on Agent type). Payload **id** fields denote the node name; they are distinct from the envelope node_id.

| type | Description | Payload fields (besides type) |
|------|-------------|-------------------------------|
| **run_start** | Agent run started (before first node_enter) | `run_id`: string (optional), `message`: string (optional, user message), `agent`: string (optional, e.g. "react", "tot", "got") |
| **node_enter** | Node run started | `id`: string (node name, e.g. "think", "act") |
| **node_exit** | Node run ended | `id`: string (node name), `result`: "Ok" or `{"Err": string}` |
| **message_chunk** | LLM message chunk | `content`: string, `id`: string (producing node name) |
| **usage** | Token usage | `prompt_tokens`: number, `completion_tokens`: number, `total_tokens`: number |
| **values** | Full state snapshot | `state`: state |
| **updates** | State after node merge | `id`: string (node name), `state`: state |
| **custom** | Custom JSON | `value`: arbitrary JSON |
| **checkpoint** | Checkpoint | `checkpoint_id`, `timestamp`, `step`, `state`, `thread_id`, `checkpoint_ns` |
| **tot_expand** | ToT expand | `candidates`: [ string, ... ] |
| **tot_evaluate** | ToT evaluate | `chosen`: number, `scores`: [ number, ... ] |
| **tot_backtrack** | ToT backtrack | `reason`: string, `to_depth`: number |
| **got_plan** | GoT plan | `node_count`, `edge_count`, `node_ids` |
| **got_node_start** | GoT node started | `id`: string |
| **got_node_complete** | GoT node completed | `id`: string, `result_summary`: string |
| **got_node_failed** | GoT node failed | `id`: string, `error`: string |
| **got_expand** | AGoT expand | `node_id`: string, `nodes_added`, `edges_added` |
| **tool_call_chunk** | Tool call arguments streamed incrementally (e.g. streaming JSON) | `call_id`: string (optional), `name`: string (optional), `arguments_delta`: string |
| **tool_call** | Complete tool call: name and full arguments | `call_id`: string (optional), `name`: string, `arguments`: object |
| **tool_start** | Tool execution started | `call_id`: string (optional), `name`: string |
| **tool_output** | Content produced by the tool (e.g. stdout); may be sent multiple times per call | `call_id`: string (optional), `name`: string, `content`: string |
| **tool_end** | Tool execution finished | `call_id`: string (optional), `name`: string, `result`: string, `is_error`: boolean |
| **tool_approval** | Tool call awaiting user approval (e.g. destructive actions) | `call_id`: string (optional), `name`: string, `arguments`: object |

Tool event order for a single call: typically **tool_call** (or **tool_call_chunk** stream) → **tool_start** → **tool_output** (zero or more) → **tool_end**; or **tool_approval** when the client must confirm before execution.

### 4.3 Example Event Messages (with envelope)

```json
{"session_id":"sess-001","event_id":0,"type":"run_start","run_id":"run-1","message":"Hello","agent":"react"}
{"session_id":"sess-001","node_id":"run-think-1","event_id":1,"type":"node_enter","id":"think"}
{"session_id":"sess-001","node_id":"run-think-1","event_id":2,"type":"message_chunk","content":"I","id":"think"}
{"session_id":"sess-001","node_id":"run-think-1","event_id":3,"type":"message_chunk","content":" don't","id":"think"}
{"session_id":"sess-001","node_id":"run-think-1","event_id":4,"type":"usage","prompt_tokens":100,"completion_tokens":62,"total_tokens":162}
{"session_id":"sess-001","node_id":"run-think-1","event_id":5,"type":"node_exit","id":"think","result":"Ok"}
```

Without the envelope, event messages **may** contain only type and payload:

```json
{"type":"run_start","run_id":"run-1","agent":"react"}
{"type":"node_enter","id":"think"}
{"type":"message_chunk","content":"Hello","id":"think"}
{"type":"node_exit","id":"think","result":"Ok"}
```

---

## 5. Message Structure: Reply

The full assistant reply for the current run is represented by a single **reply message**, usually the last message in the stream for that run.

- **Must** include the top-level field **reply** (string), the full reply text.
- **Recommended**: Also include session_id, node_id, event_id (aligned with events for the same run; event_id continues the same sequence).

Example:

```json
{"session_id":"sess-001","node_id":"run-think-1","event_id":8,"reply":"I don't have access to your device's clock ..."}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| reply | string | Yes | Full assistant reply for this run. |
| session_id | string | No | Same as for events of this run. |
| node_id | string | No | node_id of the node run that produced this reply. |
| event_id | number | No | Same monotonic sequence as event messages. |

---

## 6. Mapping to EXPORT_SPEC

This protocol uses a uniform **type + payload** shape. The mapping to the “single-key event” form in [EXPORT_SPEC.md](./EXPORT_SPEC.md) is below. Semantics match; only the serialization form differs.

| This spec type | EXPORT_SPEC key |
|----------------|-----------------|
| run_start | RunStart |
| node_enter | TaskStart |
| node_exit | TaskEnd |
| message_chunk | Messages |
| usage | Usage |
| values | Values |
| updates | Updates |
| custom | Custom |
| checkpoint | Checkpoint |
| tot_expand | TotExpand |
| tot_evaluate | TotEvaluate |
| tot_backtrack | TotBacktrack |
| got_plan | GotPlan |
| got_node_start | GotNodeStart |
| got_node_complete | GotNodeComplete |
| got_node_failed | GotNodeFailed |
| got_expand | GotExpand |
| tool_call_chunk | ToolCallChunk |
| tool_call | ToolCall |
| tool_start | ToolStart |
| tool_output | ToolOutput |
| tool_end | ToolEnd |
| tool_approval | ToolApproval |

---

## 7. Implementation Requirements

### 7.1 Sender

- **Must**: Use the type and payload defined in §4.2 for event messages; reply messages must include the `reply` field.
- **Recommended**: Include session_id, node_id, event_id on every frame; event_id monotonically increasing from 1 within a stream.
- **node_id**: Set at each node_enter and shared for the span up to the corresponding node_exit and for that run’s reply; node_id may repeat; use event_id or order to distinguish spans.

### 7.2 Receiver

- **Must**: Parse type and payload and support messages that have only type + payload (no envelope).
- **Recommended**: Support streams without event_id; for multi-session/multi-turn, merge by session_id, node_id, and event_id (or line order). When node_id repeats, use event_id or consecutive node_enter…node_exit spans to distinguish.

### 7.3 Compatibility

- Receivers **should** accept simplified output with only type + payload and legacy streams without event_id.
- Senders are **recommended** to emit the full envelope for better multi-turn and multi-session rendering.

---

## 8. Versioning and Extensions

- The envelope uses **node_id** for the node run ID (it may repeat); the event body uses **id** for the node name (or node_id in got_expand).
- When adding new event types, add a new type and payload definition and keep the “one type per frame” convention.
- This spec is aligned with EXPORT_SPEC semantics and serves as the normative format for streaming output.
