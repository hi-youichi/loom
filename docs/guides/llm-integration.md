# LLM Integration

Loom's ReAct Think node depends on an **LlmClient**: given messages, it returns assistant content and optional tool calls. This document covers the trait design, built-in implementations, streaming, and token usage.

## LlmClient trait

**LlmClient** is the single interface for LLM calls:

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// One turn: messages in, assistant content and optional tool_calls out.
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, AgentError>;

    /// Streaming: optional channel for token chunks; still returns full LlmResponse.
    async fn invoke_stream(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
    ) -> Result<LlmResponse, AgentError>;

    /// Streaming with incremental tool call arguments (tool_delta_tx).
    async fn invoke_stream_with_tool_delta(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
        tool_delta_tx: Option<mpsc::Sender<ToolCallDelta>>,
    ) -> Result<LlmResponse, AgentError>;
}
```

- **invoke**: Non-streaming; ThinkNode can use this for simple runs.
- **invoke_stream**: Same semantics, but when `chunk_tx` is `Some`, implementations send **MessageChunk** (e.g. text, thinking) as tokens arrive. Default implementation calls `invoke` and sends the full content as one chunk.
- **invoke_stream_with_tool_delta**: Optional **ToolCallDelta** stream (call_id, name, arguments_delta) for incremental tool-call parsing. Default delegates to `invoke_stream`.

## LlmResponse and types

- **LlmResponse**: `content` (assistant text), `reasoning_content` (optional), `tool_calls` (Vec&lt;ToolCall&gt;), `usage` (optional).
- **LlmUsage**: OpenAI-style `prompt_tokens`, `completion_tokens`, `total_tokens`, plus optional `prompt_tokens_details` / `completion_tokens_details` when the provider returns them — used for logging and streaming (e.g. StreamEvent::Usage only carries the three headline counts).
- **ToolCallDelta**: Incremental tool call from stream: `call_id`, `name`, `arguments_delta`.
- **MessageChunk**: Streamed content — e.g. `message(content)` or `thinking(content)` for extended thinking.

ThinkNode maps `LlmResponse` into **ReActState**: appends Assistant message, sets `tool_calls`, merges `usage` / `total_usage`.

## ChatOpenAI implementation

**ChatOpenAI** uses the OpenAI Chat Completions API (via `async_openai` when feature `openai` is enabled).

- **Config**: Typically `OPENAI_API_KEY` (and optional `OPENAI_BASE_URL`, `MODEL`). Or build with `ChatOpenAI::with_config`.
- **Tools**: Optional tools (e.g. from `ToolSource::list_tools()`) to enable tool_calls in the response. Tool choice: **ToolChoiceMode** (Auto, None, Required).
- **Streaming**: Implements `invoke_stream` (and optionally `invoke_stream_with_tool_delta`) using the streaming API; sends **MessageChunk** as tokens arrive; tool_calls are accumulated from stream chunks.
- **Thinking tags**: When `parse_thinking_tags` is enabled, content between `<think>` and `</think>` is parsed and emitted as thinking chunks; the stored assistant message can strip these tags.

Use **build_react_runner_with_openai** or the build module's LLM resolution to get a ChatOpenAI-backed runner when the config points to OpenAI.

### Automatic routing from `MODEL`

When `LLM_PROVIDER` is not set, the build layer infers provider type from `MODEL` if it is in `provider/model` format:

- `openai/...` -> OpenAI client
- non-`openai` provider prefix -> OpenAI-compatible client

In this mode, the provider prefix is used only for routing and the request model name is normalized to the part after `/`.
For OpenAI-compatible routing, `OPENAI_BASE_URL` is required at runtime. When using `~/.loom/config.toml` `[[providers]]` entries, Loom can auto-fill `OPENAI_BASE_URL` from the models.dev provider `api` field if `base_url` is omitted.

## ChatBigModel (智谱)

**ChatBigModel** is an OpenAI-compatible client for the BigModel (智谱) API (`https://open.bigmodel.cn/api/paas/v4/`).

- Same env-style config as OpenAI: `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `MODEL`; use `LLM_PROVIDER=bigmodel` to select this client.
- Implements **invoke_stream** and **invoke_stream_with_tool_delta** via SSE; parses `data:` lines and `data: [DONE]`, emits **MessageChunk** and **ToolCallDelta**.
- Retries on retryable 5xx (500, 502, 503, 504) with backoff.

## MockLlm for testing

**MockLlm** returns fixed content and optional tool_calls so you can test the graph without calling a real API.

- **MockLlm::with_get_time_call()**: One assistant message and one ToolCall (e.g. get_time) — one round think → act → observe.
- **MockLlm::with_no_tool_calls(content)**: No tool_calls — Think returns END path.
- **stateful**: First call returns (content, tool_calls), second returns (second_content, []); use for multi-round tests.
- **stream_by_char**: When set, `invoke_stream` sends each character as a separate chunk (for stream tests).
- **usage**: Optional **LlmUsage** for testing usage merge in ThinkNode.

## Streaming responses

- ThinkNode can call **invoke_stream** (or **invoke_stream_with_tool_delta**) when RunContext has streaming enabled; it sends chunks to the graph's stream (e.g. MessageChunk, ToolCallDelta).
- **StreamEvent::Messages** carries message chunks (content, metadata with loom_node).
- **StreamEvent::Usage** can carry token usage when the implementation provides it.
- SSE and WebSocket layers consume these events and expose them to clients (see [Streaming](streaming.md)).

## Token management and rate limiting

- **LlmUsage** is returned in **LlmResponse** and merged in ThinkNode into **ReActState::usage** (last call, may include details) and **total_usage** (sums only the three headline counts; detail fields are cleared on the aggregate). Used for logging and for compression/context-window logic (e.g. when to prune messages).
- Rate limiting is not implemented inside Loom; use client-side backoff, provider-specific headers, or a proxy if you need to throttle requests.

## Summary

| Component | Purpose |
|-----------|---------|
| LlmClient | Trait: invoke, invoke_stream, invoke_stream_with_tool_delta |
| LlmResponse | content, reasoning_content, tool_calls, usage |
| ChatOpenAI | OpenAI API; streaming; optional thinking-tag parsing |
| ChatBigModel | BigModel API; SSE streaming; 5xx retry |
| MockLlm | Fixed responses for tests; optional stateful and stream_by_char |

Next: [Tool System](../architecture/tool-system.md) for ToolSource, MCP, and tool execution.
