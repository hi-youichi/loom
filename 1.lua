--------------------------------------------
-- Goal:  Analyze Loom vs langgraph diffs, write docs, plan iterative alignment
-- Arch:
--   +------------------+        +------------------+
--   | discover         |=======>| discover         |
--   | langgraph        |        | loom             |
--   +------------------+        +------------------+
--        |                           |
--        v                           v
--   [langgraph_arch]           [loom_arch]
--        |                           |
--        +<==========================+
--        |
--        v
--   +------------------+        +------------------+        +------------------+
--   | analyze          |=======>| document         |=======>| plan             |
--   | diffs            |        | differences      |        | alignment        |
--   +------------------+        +------------------+        +------------------+
--        |                           |                           |
--        v                           v                           v
--   [diff_report]              [diff_doc]                 [alignment_plan]
--                                                               |
--                                                               v
--                                                  +------------------+
--                                                  | review           |
--                                                  | plan             |
--                                                  +------------------+
--                                                       |         ^
--                                                       v         |
--                                                  +------------------+
--                                                  | refine           |
--                                                  | plan             |
--                                                  +------------------+
--                                                       |
--                                                       v
--                                                  [final_plan]
--
--   Flow: discover(lg,loom) -> diff_report -> diff_doc -> alignment_plan -> review -> final
--------------------------------------------

meta = {
  reasoning = "Discover both codebases in parallel, analyze differences, document findings, create iterative alignment plan with review loop",
  phases = {
    { label = "discover-langgraph", description = "Explore langgraph architecture and core modules", agents = 1 },
    { label = "discover-loom", description = "Explore Loom equivalent modules and architecture", agents = 1, depends_on = {} },
    { label = "analyze-diffs", description = "Compare implementations and identify differences", agents = 1, depends_on = { 1, 2 } },
    { label = "document-diffs", description = "Write difference documentation", agents = 1, depends_on = { 3 } },
    { label = "plan-alignment", description = "Create iterative alignment plan with milestones", agents = 1, depends_on = { 4 } },
    { label = "review-plan", description = "Review agent critiques the alignment plan", agents = 1, depends_on = { 5 } },
    { label = "refine-plan", description = "Incorporate review feedback into final plan", agents = 1, depends_on = { 6 } },
  },
}

local LANGGRAPH_ARCH_SCHEMA = {
  type = "object",
  properties = {
    overview = { type = "string", description = "High-level architecture description" },
    core_modules = {
      type = "array",
      items = {
        type = "object",
        properties = {
          name = { type = "string" },
          path = { type = "string" },
          purpose = { type = "string" },
          key_classes = { type = "array", items = { type = "string" } },
          key_functions = { type = "array", items = { type = "string" } },
        },
        required = { "name", "path", "purpose" },
      },
    },
    state_management = { type = "string", description = "How state is managed" },
    execution_model = { type = "string", description = "How graphs are executed" },
    node_edge_model = { type = "string", description = "How nodes and edges work" },
    checkpointing = { type = "string", description = "Checkpoint/persistence mechanism" },
    streaming = { type = "string", description = "Streaming support details" },
  },
  required = { "overview", "core_modules", "state_management", "execution_model" },
}

local LOOM_ARCH_SCHEMA = {
  type = "object",
  properties = {
    overview = { type = "string" },
    core_modules = {
      type = "array",
      items = {
        type = "object",
        properties = {
          name = { type = "string" },
          path = { type = "string" },
          purpose = { type = "string" },
          key_structs = { type = "array", items = { type = "string" } },
          key_functions = { type = "array", items = { type = "string" } },
        },
        required = { "name", "path", "purpose" },
      },
    },
    state_management = { type = "string" },
    execution_model = { type = "string" },
    agent_model = { type = "string", description = "How agents/subagents work" },
    tool_integration = { type = "string", description = "How tools are integrated" },
  },
  required = { "overview", "core_modules", "state_management", "execution_model" },
}

local DIFF_SCHEMA = {
  type = "object",
  properties = {
    summary = { type = "string" },
    categories = {
      type = "array",
      items = {
        type = "object",
        properties = {
          category = { type = "string", description = "e.g. state-management, execution, streaming" },
          differences = {
            type = "array",
            items = {
              type = "object",
              properties = {
                aspect = { type = "string" },
                langgraph_approach = { type = "string" },
                loom_approach = { type = "string" },
                impact = { type = "string", enum = { "high", "medium", "low" } },
                alignment_difficulty = { type = "string", enum = { "easy", "medium", "hard" } },
              },
              required = { "aspect", "langgraph_approach", "loom_approach", "impact" },
            },
          },
        },
        required = { "category", "differences" },
      },
    },
    priority_alignment = {
      type = "array",
      items = { type = "string" },
      description = "Ordered list of what should be aligned first",
    },
  },
  required = { "summary", "categories" },
}

local DOC_SCHEMA = {
  type = "object",
  properties = {
    title = { type = "string" },
    executive_summary = { type = "string" },
    sections = {
      type = "array",
      items = {
        type = "object",
        properties = {
          heading = { type = "string" },
          content = { type = "string" },
          code_examples = { type = "array", items = { type = "string" } },
        },
        required = { "heading", "content" },
      },
    },
    conclusion = { type = "string" },
  },
  required = { "title", "executive_summary", "sections" },
}

local PLAN_SCHEMA = {
  type = "object",
  properties = {
    total_milestones = { type = "integer" },
    milestones = {
      type = "array",
      items = {
        type = "object",
        properties = {
          id = { type = "integer" },
          name = { type = "string" },
          goal = { type = "string" },
          changes = {
            type = "array",
            items = {
              type = "object",
              properties = {
                file = { type = "string" },
                description = { type = "string" },
                complexity = { type = "string", enum = { "low", "medium", "high" } },
              },
              required = { "file", "description" },
            },
          },
          test_criteria = {
            type = "array",
            items = { type = "string" },
            description = "Specific tests or checks to verify this milestone",
          },
          estimated_effort = { type = "string", description = "e.g. 2-3 days" },
          dependencies = { type = "array", items = { type = "integer" } },
        },
        required = { "id", "name", "goal", "test_criteria" },
      },
    },
    risk_assessment = { type = "string" },
    rollback_strategy = { type = "string" },
  },
  required = { "total_milestones", "milestones" },
}

local REVIEW_SCHEMA = {
  type = "object",
  properties = {
    overall_assessment = { type = "string" },
    strengths = { type = "array", items = { type = "string" } },
    weaknesses = { type = "array", items = { type = "string" } },
    suggestions = {
      type = "array",
      items = {
        type = "object",
        properties = {
          category = { type = "string", enum = { "milestone-order", "test-coverage", "risk", "scope", "granularity" } },
          suggestion = { type = "string" },
          priority = { type = "string", enum = { "must-fix", "should-fix", "nice-to-have" } },
          affected_milestones = { type = "array", items = { type = "integer" } },
        },
        required = { "category", "suggestion", "priority" },
      },
    },
    revised_milestone_order = { type = "array", items = { type = "integer" } },
    additional_tests = { type = "array", items = { type = "string" } },
  },
  required = { "overall_assessment", "suggestions" },
}

function main()
  phase("discover-langgraph", 1)
  local lg = agent({
    name = "discover-langgraph",
    description = "Explore langgraph architecture",
    prompt = [[You are analyzing the langgraph codebase vendored at thirdparty/langgraph/.

Your task:
1. Read the directory structure to understand the project layout
2. Identify core modules: state management, graph execution, nodes/edges, checkpointing, streaming
3. For each core module, note:
   - Key classes/structs and their responsibilities
   - Key functions/methods and what they do
   - How they interact with other modules
4. Understand the execution model: how does a graph run? What is the step/turn model?
5. Understand state management: how is state passed between nodes? How is it updated?
6. Understand checkpointing: how does persistence work?
7. Understand streaming: what streaming modes are supported?

Focus on Python source files under thirdparty/langgraph/. Look at:
- Core graph execution logic
- State channel implementations
- Node/edge abstractions
- Checkpoint/persistence interfaces
- Streaming implementations

Return a structured analysis of the architecture.]],
    schema = LANGGRAPH_ARCH_SCHEMA,
  })
  if not lg.ok then
    report({ error = "discover-langgraph failed: " .. (lg.status or "unknown") })
    return
  end

  phase("discover-loom", 1)
  local loom = agent({
    name = "discover-loom",
    description = "Explore Loom architecture",
    prompt = [[You are analyzing the Loom codebase (Rust project) to understand its workflow/agent execution architecture.

Your task:
1. Explore the src/ directory to find modules related to:
   - Agent execution and orchestration
   - State management between agent turns
   - Tool integration and execution
   - Streaming responses
   - Checkpointing/persistence (if any)
   - Graph/workflow execution (if any)

2. Key areas to investigate:
   - src/agent/ - agent execution logic
   - src/runtime/ - runtime pipeline
   - src/core/ - core types and state
   - src/mcp.rs - MCP integration
   - Any workflow/graph execution modules

3. For each relevant module, document:
   - Key structs and their responsibilities
   - Key functions and what they do
   - How state flows through the system
   - How tools are invoked

4. Compare conceptually with what a graph-based agent framework would need:
   - Does Loom have explicit graph/node/edge abstractions?
   - How does Loom handle multi-turn agent conversations?
   - How does Loom manage tool calls and results?
   - Does Loom support checkpointing/resumption?

Return a structured analysis of Loom's architecture focusing on aspects comparable to langgraph.]],
    schema = LOOM_ARCH_SCHEMA,
  })
  if not loom.ok then
    report({ error = "discover-loom failed: " .. (loom.status or "unknown") })
    return
  end

  phase("analyze-diffs", 1)
  local diffs = agent({
    name = "analyze-diffs",
    description = "Compare Loom and langgraph implementations",
    prompt = [[You are comparing two agent framework implementations:

## Langgraph (Python) Architecture:
]] .. json.encode(lg.output) .. [[

## Loom (Rust) Architecture:
]] .. json.encode(loom.output) .. [[

Your task:
1. Identify ALL significant implementation differences between the two frameworks
2. Categorize differences by area:
   - State management approach
   - Execution model (graph-based vs linear/other)
   - Node/edge abstractions (present vs absent)
   - Checkpointing/persistence
   - Streaming support
   - Tool integration patterns
   - Error handling and recovery
   - Multi-agent coordination
   - Memory/context management

3. For each difference:
   - Describe what langgraph does
   - Describe what Loom does (or doesn't do)
   - Assess the impact of the difference
   - Estimate alignment difficulty

4. Prioritize which differences should be aligned first based on:
   - User-facing impact
   - Implementation complexity
   - Dependencies between features

Be thorough and specific. Include code references where possible.]],
    schema = DIFF_SCHEMA,
  })
  if not diffs.ok then
    report({ error = "analyze-diffs failed: " .. (diffs.status or "unknown") })
    return
  end

  phase("document-diffs", 1)
  local doc = agent({
    name = "document-diffs",
    description = "Write difference documentation",
    prompt = [[You are writing technical documentation about the differences between Loom (Rust) and langgraph (Python) agent frameworks.

## Analysis Results:
]] .. json.encode(diffs.output) .. [[

Your task:
1. Write a comprehensive markdown document that explains:
   - Executive summary of key differences
   - Detailed comparison by category
   - Code examples showing how each framework handles key concepts
   - Implications for users migrating between frameworks

2. Structure the document with clear sections:
   - Overview/Introduction
   - Architecture Comparison
   - State Management
   - Execution Model
   - Tool Integration
   - Streaming & Real-time
   - Persistence & Recovery
   - Multi-agent Patterns
   - Summary & Recommendations

3. For each section:
   - Explain the conceptual difference
   - Show how it manifests in code (pseudo-code or actual snippets)
   - Explain the practical implications

4. Write in a technical but accessible style suitable for developers.

Return the document content structured for markdown rendering.]],
    schema = DOC_SCHEMA,
  })
  if not doc.ok then
    report({ error = "document-diffs failed: " .. (doc.status or "unknown") })
    return
  end

  phase("plan-alignment", 1)
  local plan = agent({
    name = "plan-alignment",
    description = "Create iterative alignment plan",
    prompt = [[You are creating an iterative alignment plan to bring Loom closer to langgraph's capabilities.

## Context:
- Loom is a Rust agent framework
- Langgraph is a Python graph-based agent framework
- We want to align key capabilities iteratively

## Differences to Address:
]] .. json.encode(diffs.output) .. [[

Your task:
1. Create a milestone-based plan where:
   - Each milestone is independently testable
   - Each milestone delivers visible value
   - Milestones build on each other progressively
   - No milestone requires "everything at once"

2. For each milestone, specify:
   - Clear goal (what capability is being added/aligned)
   - Specific file changes needed
   - Test criteria (how to verify it works)
   - Estimated effort
   - Dependencies on previous milestones

3. Order milestones by:
   - Foundation first (core abstractions before features)
   - Testable increments (each can be verified independently)
   - User value (earlier milestones should provide some benefit)
   - Risk management (harder/riskier changes later when foundation is solid)

4. Include:
   - Risk assessment for the overall plan
   - Rollback strategy if a milestone causes issues
   - Suggested test commands for each milestone

Aim for 4-7 milestones spanning 2-6 weeks of work. Each milestone should be completable in 3-5 days.]],
    schema = PLAN_SCHEMA,
  })
  if not plan.ok then
    report({ error = "plan-alignment failed: " .. (plan.status or "unknown") })
    return
  end

  phase("review-plan", 1)
  local review = agent({
    name = "review-plan",
    description = "Review alignment plan for improvements",
    prompt = [[You are a senior architect reviewing an alignment plan for Loom (Rust) to adopt langgraph-like capabilities.

## The Plan:
]] .. json.encode(plan.output) .. [[

## Original Differences Analysis:
]] .. json.encode(diffs.output) .. [[

Your task:
1. Critique the plan from multiple angles:
   - Is the milestone ordering optimal?
   - Are test criteria sufficient and specific?
   - Are there missing considerations (backwards compatibility, performance, etc.)?
   - Is the granularity appropriate (too coarse or too fine)?
   - Are dependencies correctly identified?
   - Are there risks not addressed?

2. Provide specific suggestions:
   - Reorder milestones if needed
   - Add missing test criteria
   - Split large milestones
   - Merge trivial milestones
   - Add missing considerations

3. Assess:
   - Overall feasibility
   - Timeline realism
   - Technical risk
   - Test coverage gaps

4. Suggest additional tests or verification steps that should be added.

Be critical but constructive. The goal is to improve the plan before implementation.]],
    schema = REVIEW_SCHEMA,
  })
  if not review.ok then
    log("review-plan had issues, proceeding with original plan", "warn")
    review.output = { overall_assessment = "Review skipped", suggestions = {} }
  end

  phase("refine-plan", 1)
  local final = agent({
    name = "refine-plan",
    description = "Incorporate review feedback",
    prompt = [[You are refining an alignment plan based on review feedback.

## Original Plan:
]] .. json.encode(plan.output) .. [[

## Review Feedback:
]] .. json.encode(review.output) .. [[

## Original Differences:
]] .. json.encode(diffs.output) .. [[

Your task:
1. Incorporate ALL "must-fix" suggestions from the review
2. Incorporate "should-fix" suggestions where practical
3. Produce a final, improved plan that:
   - Has optimal milestone ordering
   - Has comprehensive test criteria
   - Addresses identified risks
   - Has realistic timeline
   - Is ready for implementation

4. For each milestone, ensure:
   - Clear, specific goal
   - Detailed file changes
   - Comprehensive test criteria (unit tests, integration tests, manual verification)
   - Realistic effort estimate
   - Clear dependencies

5. Add a "Verification Commands" section for each milestone showing exact commands to run.

Return the final, review-incorporated plan.]],
    schema = PLAN_SCHEMA,
  })
  if not final.ok then
    log("refine-plan failed, using original plan", "warn")
    final = plan
  end

  report({
    documentation = doc.output,
    alignment_plan = final.output,
    review_feedback = review.output,
    diff_analysis = diffs.output,
  })
end