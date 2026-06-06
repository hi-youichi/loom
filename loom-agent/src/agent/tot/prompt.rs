//! ToT (Tree of Thoughts) prompt fragments for expand and evaluate nodes.

/// System addon for ThinkExpand: single strict format. The exact number of candidates (N) is
/// appended in build_messages as "Generate exactly N candidates". Do not say "2 or 3" here
/// so the model follows the dynamic N (e.g. 3).
pub const TOT_EXPAND_SYSTEM_ADDON: &str = r#"
You are in Tree-of-Thoughts mode. For the NEXT STEP ONLY, output exactly N alternative candidates (N is given in the next instruction). Use ONLY this format, one candidate per line:

CANDIDATE 1: THOUGHT: <one sentence reasoning> | TOOL_CALLS: [{"name":"tool_name","arguments":"{}"}]
CANDIDATE 2: THOUGHT: <one sentence> | TOOL_CALLS: []
CANDIDATE 3: THOUGHT: <one sentence> | TOOL_CALLS: [{"name":"other_tool","arguments":"{\"key\":\"value\"}"}]

Rules:
- You MUST output exactly N lines (CANDIDATE 1, 2, ... N). No fewer.
- THOUGHT: one short sentence. TOOL_CALLS: valid JSON array; use [] if no tools.
- Include at least one candidate that uses tools when the task needs them. Choose tools that fit the task:
  - Search / how-to / research: web_fetcher or web_search_exa. Example: [{"name":"web_fetcher","arguments":"{\"url\":\"https://...\"}"}]
  - Clone repo, run commands, or local files: bash. Example: [{"name":"bash","arguments":"{\"command\":\"git clone https://github.com/org/repo.git\"}"}]
  - Other tasks: use [] or the tool that matches (read, etc.).
"#;

/// Addon for research/how-to tasks: require multiple tool calls and structured answer.
/// Append to system or expand prompt when task is research-like.
pub const TOT_RESEARCH_QUALITY_ADDON: &str = r#"
For "how to", "research", or look-up questions: run at least 2–3 tool calls (e.g. search) before giving a final answer. Structure the answer step-by-step (from simple to in-depth) and cite or mention sources when possible.
"#;
