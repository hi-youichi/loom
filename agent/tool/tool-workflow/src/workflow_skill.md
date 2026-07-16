---
name: workflow
description: Lua DSL reference for writing multi-agent workflows
triggers:
  - workflow
  - multi-agent
  - lua script
  - workflow_list
  - workflow_status
  - workflow_events
  - debug workflow
  - workflow failed
  - workflow status
metadata:
  conditions:
    requires_tools:
      - workflow_start
      - workflow_status
tags:
  - workflow
  - orchestration
  - lua
  - instance
---

# Workflow DSL Reference

Use this when writing or debugging Lua workflow scripts passed to `workflow_start`.

## 1. Which tool to use

The workflow surface has six focused tools:

| Intent | Tool | Minimum args |
| --- | --- | --- |
| Start a new multi-agent task | `workflow_start` | `script` or `workflow` |
| Find a completed instance | `workflow_list` | optional `limit`, `cursor`, `status_filter` |
| Check one instance | `workflow_status` | `instance_dir` |
| Inspect detailed execution events | `workflow_events` | `instance_dir` |
| View the captured Lua source | `workflow_source` | `instance_dir` |
| Find available workflow definitions | `workflow_files` | none |

After `workflow_start`, use this exact sequence:

```text
workflow_start
→ sleep 5
→ workflow_status
→ repeat only while status == "running"
```

Use a shell wait between status calls. On PowerShell use `Start-Sleep -Seconds 5`; on shells with the standard command use `sleep 5`. Do not poll in a tight loop or issue the wait and status calls in parallel.

Use `workflow_list` when the instance identifier is unavailable. Use `workflow_events` only after `workflow_status` identifies a failure or suspicious phase. Use `workflow_source` when reviewing the executed Lua is relevant. `workflow_files` lists definitions available to start; it is not a workflow-result inspection tool.

All workflow tools provide their results directly. Do not use a file-reading tool to follow execution, find reports, or inspect outputs.

## 2. Execution model

- The Lua script is a pure orchestrator. The sandbox disables `io`, `os`, and `require`.
- Real work happens inside `agent()` prompts; the script does not access tools directly.
- `report(value)` is the workflow result.
- `workflow_start` returns before agents finish; `workflow_status` is the source of truth for the public lifecycle state.

## 3 Minimal skeleton

```lua
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
  phase("process")
  phase("report")
  report({ result = ... })
end
```

For the full Lua DSL reference, see `references/dsl-reference.md`. For schemas, return shapes, and diagnostics, see `references/tool-usage.md`. Load these through the skill tool when needed.

## Additional resources

- `references/architecture-header.md`
- `references/agent-prompts.md`
- `references/task-decomposition.md`
- `references/adversarial-verification.md`
- `references/examples.md`
