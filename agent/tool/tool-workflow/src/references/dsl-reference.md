# Lua DSL Reference

This is the detailed reference moved out of the main workflow skill. For
action selection see `workflow_skill.md` §1; for per-action schemas, return
shapes, and the three-step diagnostic flow, see
`references/tool-usage.md`. Load these via the skill tool when needed.

## Primitives

### agent(opts) -> result

Run one subagent to completion.

**opts:**

| field          | type   | required | description                                              |
| -------------- | ------ | -------- | -------------------------------------------------------- |
| `prompt`       | string | yes      | Instructions for the subagent                            |
| `schema`       | table  | no       | JSON Schema constraining the agent's structured output   |
| `model`        | string | no       | Override the model for this agent                        |
| `name`         | string | no       | Short identifier for CLI display                         |
| `description`  | string | no       | One-line CLI description                                 |
| `timeout_ms`   | int    | no       | Per-agent timeout                                        |

**result:**

| field      | type        | description                                                  |
| ---------- | ----------- | ------------------------------------------------------------ |
| `ok`       | bool        | True if the agent succeeded                                  |
| `status`   | string      | `"ok"` / `"error"` / `"cancelled"` / `"timed_out"`           |
| `output`   | table       | Parsed agent response (Lua table)                            |
| `tokens`   | int         | Token usage                                                  |
| `findings` | array?      | Accumulated findings (when applicable)                       |

**Schema rules:**

- Always check `result.ok` before using `result.output`.
- **Analysis agents** (extract / analyze / verify): MUST provide a schema. JSON-mode lets the orchestrator consume structured fields reliably.
- **Execution agents** (write / edit / refactor): omit the schema or use a minimal one like `{changed=bool, files=string[]}`. A rich schema forces JSON-mode and **prevents tool calls**, which execution agents need.
- Put concrete context in `prompt`: file paths, module names, acceptance criteria. Don't make the agent guess.

### parallel(items, mapFn) -> result[]

**Barrier fan-out**: runs all items concurrently, waits for **all** to finish.

```lua
local results = parallel(items, function(item)
  return { prompt = "Process: " .. item.id, schema = ITEM_SCHEMA }
end)
-- results[i] corresponds to items[i]
```

Use when you need every result before continuing.

### pipeline{ items=, stages=, max_inflight= } -> { items=, ok=, failed= }

**Streaming multi-stage**: items flow through all stages; different items can be in different stages simultaneously.

```lua
local out = pipeline {
  items = urls,
  max_inflight = 4,
  stages = {
    function(item)
      local r = agent({ prompt = "Fetch " .. item.url })
      return { url = item.url, body = r.output.body, ok = r.ok }
    end,
    function(prev)
      if not prev.ok then return prev end   -- error degradation
      local r = agent({ prompt = "Summarize: " .. prev.body, schema = SUM_SCHEMA })
      return { url = prev.url, summary = r.output.summary, ok = r.ok }
    end,
  },
}
```

Each stage handler MUST call `agent()` itself. Stage 1 receives the raw item; later stages receive the previous stage's return value.

Error degradation: check `prev.ok` at handler start; on failure, return default data directly (skip agent call).

### phase(name, planned?) -> phase_id

Declares a progress phase. Emits a CLI-visible event.

```lua
phase("analyze", 8)    -- named "analyze", ~8 units of work planned
```

### phase_begin(name) / phase_end(span)

Span-based phase timing. Must be paired.

### workflow(path, args?) -> result

Calls another saved `.lua` workflow as a sub-step. Use to compose larger workflows from smaller ones.

### report(value)

Sets the final output. **First call wins**; later calls are silently ignored. Always `return` after an error report:

```lua
if not r.ok then
  report({ error = "phase X failed: " .. r.status })
  return
end
```

### log(msg, level?)

`level`: `"info"` (default) / `"warn"` / `"error"`.

### budget(time_ms?, max_rounds?)

Hint resource limits for the current phase. Soft limits — the engine may exceed.

### json.encode(value) / json.decode(string)

Round-trip between Lua tables and JSON strings.

## Globals

- `args` — user-supplied arguments table (from the tool's `args` field)
- `ctx`  — run context; `ctx.run_id` is the current run ID (string)

## Error Handling

- ALWAYS check `result.ok` before using `result.output`.
- On failure: `log()` the error, then decide — skip, retry, or abort with `report()`.
- Always `return` after an error `report()` to prevent nil dereference.
- Graceful degradation: when a stage fails, feed a minimal/default prompt to the next stage rather than crashing the pipeline.

## Rules

1. Script orchestrates only — no filesystem / shell access from script.
2. `report()` exactly once. Always `return` after an error report.
3. Fan-out bounded: max ~16 concurrent agents. For larger sets, have an agent enumerate targets first, then loop.
4. `pipeline()` for multi-stage streaming; `parallel()` for gather-all barrier.
5. Schema usage:
   - Analysis (extract / analyze / verify): MUST provide schema.
   - Execution (write / edit / refactor): omit or minimal schema.
6. Use `phase()` / `log()` to make progress legible in the CLI.
7. For file-writing tasks: tell the agent which tool to use (e.g. `Write`, `str_replace_based_edit_tool`) and the exact file path.
8. Double-quote all string values, especially non-ASCII text — Lua syntax requires it.
