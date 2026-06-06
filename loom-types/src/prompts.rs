//! Shared prompt constants used across loom and loom-agent crates.

/// Default ReAct **base** system prompt when no `react.yaml` / `REACT_SYSTEM_PROMPT` override.
///
/// The previous RULES/PHASES block (THOUGHT / FINAL_ANSWER / "do not call tools" for in-knowledge
/// questions) is **disabled**: it conflicts with `tool_choice: required` and with tasks that need
/// real workspace listing without hallucination.
///
/// **Restore:** copy `system_prompt` from `loom/prompts/experimental/react.yaml` into
/// `loom/prompts/react.yaml`, or set env `REACT_SYSTEM_PROMPT`. Role / AGENTS.md / Helve sections
/// still apply on top of this empty base.
pub const REACT_SYSTEM_PROMPT: &str = "";

/// DUP understand prompt
pub const DUP_UNDERSTAND_PROMPT: &str = r#"You are an understanding module. Your job is to analyze the user's request and output a structured understanding.

Output format (JSON only, no extra text):
{
  "core_goal": "one sentence describing what the user wants to achieve",
  "key_constraints": ["constraint 1", "constraint 2"],
  "relevant_context": "brief summary of workspace, files, or context that matters"
}

Be concise. Do not execute any actions. Only extract and structure the understanding."#;

/// ToT expand system addon
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

/// ToT research quality addon
pub const TOT_RESEARCH_QUALITY_ADDON: &str = r#"
For "how to", "research", or look-up questions: run at least 2–3 tool calls (e.g. search) before giving a final answer. Structure the answer step-by-step (from simple to in-depth) and cite or mention sources when possible.
"#;

/// AGOT expand system
pub const AGOT_EXPAND_SYSTEM: &str = r#"You are a task decomposer. A complex sub-task has just been executed. You must break it down into 2-6 smaller sub-tasks that can be executed next.

Rules:
- Output ONLY valid JSON, no markdown or explanation.
- Format: {"nodes": [{"id": "step1", "description": "..."}, ...], "edges": [["parent_id", "step1"], ["step1", "step2"], ...]}
- Use short node ids: step1, step2, sub_a, sub_b, etc. (the parent_id will be prefixed automatically).
- Edges: at least one edge must go FROM the parent node (given in the user message) TO a new node.
- Edges between new nodes are allowed. The graph must be a DAG (no cycles).
- Descriptions should be concrete and actionable.
- Build on the parent's result when relevant.
"#;

/// GOT plan system
pub const GOT_PLAN_SYSTEM: &str = r#"You are a task planner. Given a user request, you must decompose it into a directed acyclic graph (DAG) of sub-tasks.

Rules:
- Output ONLY valid JSON, no markdown or explanation.
- Format: {"nodes": [{"id": "unique_id", "description": "what to do"}], "edges": [["from_id", "to_id"]]}
- Each edge means: "from_id" must complete before "to_id" can start.
- Use short, unique node ids (e.g. read_a, read_b, merge, report).
- Keep 2-8 nodes. Edges must form a DAG (no cycles).
- Descriptions should be clear and actionable for an assistant that can use tools.
"#;