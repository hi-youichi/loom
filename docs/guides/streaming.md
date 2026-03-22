# Streaming & Communication

Loom supports streaming graph execution: per-step state, task events, checkpoints, message chunks, and custom payloads. This document covers **StreamMode**, **StreamEvent**, **StreamWriter**, the protocol bridge, and WebSocket/SSE usage.

## State streaming implementation

**CompiledStateGraph::stream(state, config, stream_mode)** runs the same execution loop as **invoke**, but sends events over a channel. The caller receives a **ReceiverStream&lt;StreamEvent&lt;S&gt;&gt;** (e.g. from `tokio::sync::mpsc`). Events are emitted after each node according to the enabled **StreamMode**:

- **Values**: Full state snapshot after each node.
- **Updates**: Incremental update with node_id and state after that node.
- **Messages**: Message chunks (e.g. from ThinkNode LLM streaming); includes **StreamMetadata** (loom_node).
- **Custom**: Custom JSON from nodes or tools (via **StreamWriter** or **RunContext::emit_custom**).
- **Checkpoints**: Checkpoint events when a checkpoint is created (requires checkpointer and config.thread_id).
- **Tasks**: TaskStart and TaskEnd for each node.
- **Tools**: Tool lifecycle (tool_call, tool_start, tool_output, tool_end, tool_approval).
- **Debug**: Enables Checkpoints and Tasks together.

## StreamEvent and StreamWriter

**StreamEvent&lt;S&gt;** variants include **Values(S)**, **Updates { node_id, state }**, **Messages { chunk, metadata }**, **Custom(Value)**, **Checkpoint(CheckpointEvent&lt;S&gt;)**, **TaskStart/TaskEnd**, **Usage**, and tool-related events. Nodes that receive **RunContext** can get a **StreamWriter** via **ctx.stream_writer()** and call **emit_custom(value)** or **emit_message(content, node_id)**; events are sent only when the corresponding **StreamMode** is enabled.

**ToolStreamWriter** is a type-erased writer for tools (no state type); use for progress or custom JSON from inside **ToolCallContext**.

## SSE (Server-Sent Events)

OpenAI-compatible SSE is provided by the **openai_sse** module: **StreamToSse**, **ChatCompletionChunk**, **parse_chat_request**, **write_sse_line**. Stream events can be converted to SSE chunks for chat-completion-style APIs. See **StreamToSse** and protocol docs for mapping **StreamEvent** to SSE.

## WebSocket-based remote execution

The **protocol** module defines WebSocket message types for remote mode:

- **ClientRequest**: RunRequest, ToolsListRequest, ToolShowRequest, PingRequest.
- **ServerResponse**: RunStreamEventResponse, RunEndResponse, ToolsListResponse, ToolShowResponse, PongResponse, ErrorResponse.

Streaming output is sent as **RunStreamEventResponse**; each event is serialized via **stream_event_to_protocol_format** (or **stream_event_to_protocol_envelope**) from **protocol::stream**. The **Envelope** format carries a type and payload so the client can dispatch by event type.

## Event streaming API (protocol::stream)

- **stream_event_to_protocol_format** / **stream_event_to_protocol_value**: Convert **StreamEvent&lt;S&gt;** to the protocol representation (e.g. JSON).
- **stream_event_to_protocol_envelope**: Wrap in **Envelope** (type + payload).
- **ProtocolEvent**, **ProtocolEventEnvelope**: Protocol-level event types used in RunStreamEventResponse.

## Streaming hooks and callbacks

- **RunContext::stream_tx** and **stream_mode**: Set by the runner when using **stream()**; nodes check **ctx.is_streaming_mode(StreamMode::Custom)** etc. before emitting.
- **RunContext::emit_custom** / **emit_message**: Convenience methods that use the context’s StreamWriter.
- ReAct **runner.stream_with_config** (and **run_react_graph_stream**) accept an optional callback **FnMut(StreamEvent)** for processing events in-process; the same events are also sent on the channel when used from **stream()**.

## Summary

| Topic | Key types / APIs |
|-------|------------------|
| Modes | StreamMode: Values, Updates, Messages, Custom, Checkpoints, Tasks, Tools, Debug |
| Events | StreamEvent, CheckpointEvent, MessageChunk, StreamMetadata |
| Writer | StreamWriter, ToolStreamWriter; RunContext::emit_custom, emit_message |
| Protocol | protocol::stream (Envelope, stream_event_to_protocol_*); ClientRequest, ServerResponse |
| SSE | openai_sse::StreamToSse, ChatCompletionChunk |

Next: [CLI Tool](cli.md) for command-line interface and profile management.
