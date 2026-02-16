# Agent prompts (canonical source)

**All default prompt text for agent patterns lives here.** The crate embeds these YAML files at compile time and uses them when no `PROMPTS_DIR` or runtime directory is present.

- `react.yaml` — ReAct system prompt, tool/execution error templates
- `tot.yaml` — ToT expand addon, research quality addon
- `got.yaml` — GoT plan_system, agot_expand_system
- `dup.yaml` — DUP understand_prompt
- `helve.yaml` — Helve workdir template (`{workdir}`), approval_destructive, approval_always

To override at runtime: copy this directory to your project as `prompts/` or set `PROMPTS_DIR`.
