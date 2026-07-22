# Adversarial Verification Pattern

When the task needs **cross-checked / verified results**, implement adversarial
verification directly in Lua using `agent()` and `parallel()`.

## Steps

1. **PRODUCE** — run producer agents (via `parallel`) on each item to generate findings.
2. **CHALLENGE** — for each finding, run adversary agents that attempt to refute it.
3. **VOTE** — keep only findings whose approval rate ≥ your threshold (e.g. 0.7).
4. **ITERATE** — feed surviving findings back as items; repeat up to N rounds.
5. **STOP** when converged (no findings refuted) or max rounds reached.

This is a **pattern**, not a primitive — write the loop in Lua. Only use it when
the task genuinely requires cross-checking; skip it for simple tasks.

## Skeleton

```lua
local ROUNDS = 3
local THRESHOLD = 0.7

local findings = initial_findings        -- from a PRODUCE phase
for round = 1, ROUNDS do
  local votes = parallel(findings, function(f)
    local challenge = agent({
      prompt = "Try to refute this finding. Be skeptical.\n"
            .. json.encode(f),
      schema = VOTE_SCHEMA,              -- { refuted: bool, reason: string }
    })
    return { finding = f, refuted = challenge.output.refuted }
  end)

  findings = {}
  for _, v in ipairs(votes) do
    local approvals = votes
      | select_where(function(x) return not x.refuted end)
    -- or compute approval rate by re-voting; simplified here
    if not v.refuted then
      table.insert(findings, v.finding)
    end
  end

  if #findings == 0 then break end       -- converged
end
```

## IMPORTANT

Fan-out must stay bounded. For adversarial voting, **batch the voter calls
with `parallel()` at the ITEM level** so the runtime can manage concurrency — do
NOT serialize voters in a nested `for` loop.