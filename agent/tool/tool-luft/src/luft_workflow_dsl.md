---
name: luft-workflow-dsl
description: Lua DSL reference for writing Luft multi-agent workflows
triggers:
  - luft
  - workflow
  - multi-agent
  - lua script
metadata:
  conditions:
    requires_tools:
      - luft
tags:
  - luft
  - workflow
  - orchestration
---

# Luft Workflow DSL Reference

Use this when writing or debugging Lua workflow scripts passed to the `luft` tool.

## Execution Model

- The Lua script is a **pure orchestrator**. The sandbox disables `io`, `os`, `require`.
- All real work (file I/O, grep, edit, web search) happens inside `agent()` prompts — the subagents have tools, the script does not.
- `report(value)` is the only output returned to the caller.

## Required Structure

Every script must contain exactly these three pieces:

```lua
meta = {                              -- required, first statement
  reasoning = "<one-line strategy>",
  phases = {
    { label = "<name>", description = "<cli text>", dynamic = false },
  },
}
function main()                      -- required, entry point
  -- orchestration logic
  report(result)                     -- required, call EXACTLY once
end
```

`report` is first-call-wins — later calls are silently ignored. Always `return` after an error `report` to prevent fall-through.

## Primitives

### agent(opts) -> result

Run one subagent to completion.

**opts:**

| field | type | required | description |
|---|---|---|---|
| `prompt` | string | yes | Instructions for the subagent |
| `schema` | table | no | JSON Schema constraining the agent's structured output |
| `model` | string | no | Override the model for this agent |
| `name` | string | no | Short identifier for CLI display |
| `description` | string | no | One-line CLI description |
| `timeout_ms` | int | no | Per-agent timeout |

**result:**

| field | type | description |
|---|---|---|
| `ok` | bool | True if the agent succeeded |
| `status` | string | `"ok"` / `"error"` / `"cancelled"` / `"timed_out"` |
| `output` | table | Parsed agent response (Lua table) |
| `tokens` | int | Token usage |
| `findings` | array? | Accumulated findings (when applicable) |

**Rules:**

- Always check `result.ok` before using `result.output`.
- Analysis agents (extract / analyze / verify): **MUST** provide a schema. JSON-mode lets the orchestrator consume structured fields reliably.
- Execution agents (write / edit / refactor): omit the schema or use a minimal one like `{changed=bool, files=string[]}`. A rich schema forces JSON-mode and **prevents tool calls**, which execution agents need.
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

## Rules

1. Script orchestrates only — no filesystem / shell access from script.
2. `report()` exactly once. Always `return` after an error report.
3. Check `result.ok` before using `result.output`.
4. Fan-out bounded: max ~16 concurrent agents. For larger sets, have an agent enumerate targets first, then loop.
5. `pipeline()` for multi-stage streaming; `parallel()` for gather-all barrier.
6. Schema usage:
   - Analysis (extract / analyze / verify): MUST provide schema.
   - Execution (write / edit / refactor): omit or minimal schema.
7. Use `phase()` / `log()` to make progress legible in the CLI.
8. For file-writing tasks: tell the agent which tool to use (e.g. `Write`, `str_replace_based_edit_tool`) and the exact file path.
9. Double-quote all string values, especially non-ASCII text — Lua syntax requires it.
10. The orchestrator waits for `report()`. Don't exit `main()` without calling it.

## Example: analyze → report

```lua
meta = {
  reasoning = "Single-pass analysis",
  phases = {
    { label = "analyze", description = "scan for issues" },
    { label = "report" },
  },
}

local SCHEMA = {
  type = "object",
  properties = {
    summary = { type = "string" },
    issues  = { type = "array", items = { type = "string" } },
  },
  required = { "summary" },
}

function main()
  phase("analyze")
  local r = agent({
    prompt = "Analyze src/auth/ for security issues. "
          .. "List each with file path and line number.",
    schema = SCHEMA,
  })
  if not r.ok then
    report({ error = "analysis failed: " .. r.status })
    return
  end
  report({ summary = r.output.summary, issues = r.output.issues })
end
```