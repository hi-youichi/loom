//! Lua script constants for resume tests.

pub const SCRIPT_3PHASE: &str = r#"
function main()
  phase("collect")
  local r = agent({name = "a1", prompt = "prompt-1"})
  if not r.ok then report({error = "a1 failed"}) return end

  phase("analyze")
  local r = agent({name = "a2", prompt = "prompt-2"})
  if not r.ok then report({error = "a2 failed"}) return end

  phase("report")
  local r = agent({name = "a3", prompt = "prompt-3"})
  if not r.ok then report({error = "a3 failed"}) return end

  report({ok = true})
end
"#;

#[allow(dead_code)]
pub const SCRIPT_MULTI_AGENT: &str = r#"
function main()
  phase("research")
  agent({name = "a1", prompt = "prompt-1"})
  agent({name = "a2", prompt = "prompt-2"})
  agent({name = "a3", prompt = "prompt-3"})

  phase("write")
  agent({name = "a4", prompt = "prompt-4"})

  report({ok = true})
end
"#;

pub const SCRIPT_NO_AGENT: &str = r#"
function main()
  phase("setup")
  phase("done")
  report({ok = true})
end
"#;
