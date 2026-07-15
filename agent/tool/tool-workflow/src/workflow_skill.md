---
name: workflow
description: Lua DSL reference for writing multi-agent workflows
triggers:
  - workflow
  - multi-agent
  - lua script
  - list-instances
  - instance-summary
  - debug workflow
  - workflow failed
  - workflow status
metadata:
  conditions:
    requires_tools:
      - workflow
tags:
  - workflow
  - orchestration
  - lua
  - instance
---

# Workflow DSL Reference
Use this when writing or debugging Lua workflow scripts passed to the
`workflow` tool.

## 1 When to use which action

The workflow tool exposes six actions. Pick the smallest one that answers
your question; escalate only when the smaller one is not enough.

| Intent                                 | Action             | Minimum args                                              |
| -------------------------------------- | ------------------ | --------------------------------------------------------- |
| Run a new multi-agent task             | `execute`          | `{"workflow": "refactor"}` or `{"script": "..."}`        |
| See what scripts exist on disk         | `list-workflows`   | `{}`                                                      |
| Review past executions / find failures | `list-instances`   | `{"limit": 20}` or `{"status_filter": "failed"}`         |
| Inspect one execution (always first)   | `instance-summary` | `{"instance_dir": "loom-instance_<ts>"}`                  |
| Drill into the event timeline          | `instance-events`  | `{"instance_dir": "...", "types": ["agent_done"]}`        |
| Read the Lua script that ran           | `instance-source`  | `{"instance_dir": "loom-instance_<ts>"}`                  |

**Progressive disclosure.** When debugging, always follow this order:
`list-instances` → `instance-summary` → `instance-events`. The summary tells
you which agents failed and which event types look anomalous; only then read
the raw event stream. Jumping straight to `instance-events` dumps hundreds of
events into your context and is almost always the wrong first move.

## 2 Execution model

- The Lua script is a **pure orchestrator**. The sandbox disables `io`, `os`, `require`.
- All real work (file I/O, grep, edit, web search) happens inside `agent()` prompts — subagents have tools, the script does not.
- `report(value)` is the only output returned to the caller.

## 3 Minimal skeleton

```lua
--------------------------------------------
-- Goal:  <one-line objective>
-- Arch:
--   discover ==> process ==> report
-- Flow:  discover -> items[] -> results -> report
--------------------------------------------
meta = {
  reasoning = "...",
  phases = {
    { label = "discover" },
    { label = "process", dynamic = true },
    { label = "report" },
  },
}
local SCHEMA = { ... }
function main()
  phase("discover")
  -- ...
  report({ result = ... })
end
```

> For the full Lua DSL reference (primitives, schema rules, error handling), see references/dsl-reference.md. For per-action schemas, return shapes, and the three-step diagnostic flow, see references/tool-usage.md. Load these via the skill tool when needed.
