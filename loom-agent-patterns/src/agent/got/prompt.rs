//! GoT PlanGraph node prompt: instruct LLM to output a DAG of sub-tasks.

/// System prompt for the PlanGraph node: decompose the user task into a DAG.
///
/// The LLM must respond with valid JSON: `{"nodes": [{"id": "...", "description": "..."}], "edges": [["from_id", "to_id"]]}`.
/// The graph must be acyclic. Node ids must be unique.
pub const GOT_PLAN_SYSTEM: &str = r#"You are a task planner. Given a user request, you must decompose it into a directed acyclic graph (DAG) of sub-tasks.

Rules:
- Output ONLY valid JSON, no markdown or explanation.
- Format: {"nodes": [{"id": "unique_id", "description": "what to do"}], "edges": [["from_id", "to_id"]]}
- Each edge means: "from_id" must complete before "to_id" can start.
- Use short, unique node ids (e.g. read_a, read_b, merge, report).
- Keep 2-8 nodes. Edges must form a DAG (no cycles).
- Descriptions should be clear and actionable for an assistant that can use tools.
"#;

/// System prompt for AGoT dynamic expansion: decompose a complex node into sub-tasks.
///
/// The LLM receives the parent node's id, description, result, and task goal.
/// It must output JSON compatible with PlanGraph format. New node ids are
/// short suffixes (e.g. "step1", "step2"); the caller prefixes them with
/// parent_id to avoid collisions.
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
