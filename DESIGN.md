# Sub-Agent Output Display - Design Document

## Problem Statement

When using `invoke_agent` tool in CLI, sub-agent events are not displayed to users. Users only see:
```
CALL invoke_agent: explore
DONE invoke_agent: completed
```

But sub-agent's thinking process, tool calls, and outputs are invisible.

## Root Cause

The `on_event` callback was created via `ctx.and_then(|c| c.any_stream_event_sender.clone()).map(...)`.
In CLI mode, `any_stream_event_sender` is `None`, so `on_event` is also `None`, meaning **all sub-agent
events are discarded**.

## Solution

### Core Fix: Always create `on_event`, buffer events, print after completion

```rust
// BEFORE (broken): on_event is None when any_stream_event_sender is None
let on_event = ctx.and_then(|c| c.any_stream_event_sender.clone()).map(|sender| {
    move |event| { sender(AnyStreamEvent::React(event)); }
});

// AFTER (fixed): on_event is always Some
let any_sender = ctx.and_then(|c| c.any_stream_event_sender.clone());
let buffer = Arc::new(Mutex::new(Vec::<String>::new()));
let buffer_clone = buffer.clone();

let on_event = Some(move |event| {
    // Buffer formatted event for later display
    if let Some(formatted) = format_subagent_event(&event, &agent_name, depth, start) {
        buffer_clone.lock().unwrap().push(formatted);
    }
    // Forward to sender if available (ACP mode)
    if let Some(sender) = &any_sender {
        sender(AnyStreamEvent::React(event));
    }
});

// After sub-agent completes, print buffer
{
    let lines = buffer.lock().unwrap();
    for line in lines.iter() {
        eprintln!("{}", line);
    }
}
```

### Why buffer instead of real-time print?

The CLI spinner uses `\r` to overwrite the current stderr line. Real-time `eprintln!` would
interleave with the spinner, causing garbled output. Buffering ensures clean display after
the sub-agent completes and before ToolEnd clears the spinner.

### Event Formatting (`format_subagent.rs`)

Formats significant events with visual hierarchy:
- **Indentation**: 2 spaces per nesting level
- **Prefix**: `↳ [agent-name]`
- **Dim text**: Thinking content uses ANSI dim
- **Tool tracking**: `Running: tool_name` / `✓ tool_name` / `✗ tool_name`

### Event Types Displayed

| Event | Format |
|-------|--------|
| `TaskStart` | `↳ [explore] Starting...` |
| `TaskEnd` | `↳ [explore] done (3.2s)` |
| `Updates { think }` | `↳ [explore] Thinking: truncated text...` |
| `Updates { act }` | `↳ [explore] Calling: read, grep` |
| `Updates { observe }` | `↳ [explore] 3 tool(s) completed` |
| `ToolStart` | `↳ [explore] Running: read` |
| `ToolEnd` | `↳ [explore] ✓ read` or `✗ read` |
| `Messages { Thinking }` | `↳ [explore] dim text...` |
| `Messages { Message }` | `↳ [explore] content text` |

### UTF-8 Safety

`truncate()` uses `chars()` instead of byte slicing to avoid panics on multi-byte characters.

### Timing

`Instant::now()` is captured once when the closure is created, passed to `format_subagent_event()`
as a parameter. `TaskEnd` computes elapsed time from this shared start point.

## Files Changed

1. `loom/src/stream_display/format_subagent.rs` - Event formatter (rewritten)
2. `loom/src/stream_display/mod.rs` - Module exports
3. `loom/src/tools/invoke_agent.rs` - Both `call_single_exec` and `invoke_single_agent`
4. `loom/src/stream_display/spinner.rs` - Fix non-exhaustive match (pre-existing)
