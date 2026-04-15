# OpenAI Prompt Engineering Practical Guide

> Curated from official OpenAI documentation, applicable to Agent development scenarios (Loom, XAgent, etc.)

---

## Table of Contents

1. [Prompt Structure](#1-prompt-structure)
2. [Message Role Hierarchy](#2-message-role-hierarchy)
3. [GPT-4.1 Core Feature: Literal Instruction Following](#3-gpt-41-core-feature-literal-instruction-following)
4. [Three Keys of Agentic Prompts](#4-three-keys-of-agentic-prompts)
5. [Tool Definition Best Practices](#5-tool-definition-best-practices)
6. [Chain of Thought Guidance](#6-chain-of-thought-cot-guidance)
7. [Long Context Techniques (1M tokens)](#7-long-context-techniques-1m-tokens)
8. [Delimiter Selection Guide](#8-delimiter-selection-guide)
9. [Prompt Debugging Workflow](#9-prompt-debugging-workflow)
10. [Few-shot Examples](#10-few-shot-examples)
11. [Reasoning Models vs GPT Models](#11-reasoning-models-vs-gpt-models)
12. [GPT-5 New Strategies](#12-gpt-5-new-strategies)
13. [Diff Format (Code Agent Specific)](#13-diff-format-code-agent-specific)
14. [Practical Examples](#14-practical-examples)

---

## 1. Prompt Structure

OpenAI's recommended prompt skeleton:

```markdown
# Role and Objective        ← Identity and goal
# Instructions              ← Core rules
## Sub-categories            ← More granular categorized rules
# Reasoning Steps           ← Reasoning/workflow steps
# Output Format             ← Output format requirements
# Examples                  ← Few-shot examples
# Context                   ← Injected external context (RAG, etc.)
# Final instructions        ← Closing instructions + CoT trigger
```

**Key Principle**: Each section has a clear Markdown heading so the model knows what it's reading.

---

## 2. Message Role Hierarchy

OpenAI API's three roles have a clear priority chain:

```
developer > user > assistant
```

| Role | Purpose | Analogy |
|------|---------|---------|
| `developer` | Business logic and rules | Function definition |
| `user` | Specific inputs | Function parameters |
| `assistant` | Historical replies | Call history |

**Application**: Put immutable workflow rules in system prompt, and variable task descriptions in user messages.

---

## 3. GPT-4.1 Core Feature: Literal Instruction Following

This is the biggest behavioral change in the GPT-4.1 series: **the model follows your instructions very literally, rather than "inferring" your intent like previous generations**.

### Advantages
- Model is highly controllable
- A single clear correction instruction is usually sufficient to change behavior

### Trade-offs
- Must write out things that were previously "implicit expectations"
- Model may not proactively supplement information

### Practical Recommendations

1. **Check for conflicting instructions** - Model tends to obey the one closest to the end
2. **Make behavioral expectations explicit** - Write down anything you "think the model should know"
3. **Avoid repetitive phrasing** - If you provide sample phrases, add "please vary your wording"
4. **No tricks needed** - No need for all-caps, bribes, tips—GPT-4.1 may over-focus on these decorations

---

## 4. Three Keys of Agentic Prompts

OpenAI verified on SWE-bench that adding just these three reminders improved pass rate by approximately **20%**:

### 1. Persistence

Tell the model it's in a multi-turn interaction and should not exit early:

```markdown
You are an agent - please keep going until the user's query is
completely resolved, before ending your turn and yielding back
to the user. Only terminate your turn when you are sure that
the problem is solved.
```

### 2. Tool-calling

Force the model to use tools to gather information, not guess:

```markdown
If you are not sure about file content or codebase structure
pertaining to the user's request, use your tools to read files
and gather the relevant information: do NOT guess or make up
an answer.
```

### 3. Planning

Make the model do explicit reasoning before and after each tool call:

```markdown
You MUST plan extensively before each function call, and reflect
extensively on the outcomes of the previous function calls.
DO NOT do this entire process by making function calls only,
as this can impair your ability to solve the problem and think
insightfully.
```

---

## 5. Tool Definition Best Practices

**Always pass tool definitions via the API's `tools` field**—never manually inject tool schemas into the system prompt and parse them yourself. OpenAI internal testing shows the native API field improves accuracy by approximately **2%** over manual injection.

### Tool Definition Standards

| Field | Requirement |
|------|-------------|
| `name` | Use clear verb+noun naming, e.g., `lookup_policy_document` |
| `description` | Detailed but concise explanation of purpose |
| `parameters` | Add description for each parameter |

> If tool calls are complex, place examples in the `# Examples` section of the system prompt, not in the description field.

---

## 6. Chain of Thought (CoT) Guidance

GPT-4.1 is not a reasoning model and has no internal chain of thought, but you can induce "thinking out loud" through prompting.

### Basic Guidance

```markdown
First, think carefully step by step about what is needed.
Then, summarize your analysis.
Then, provide your final answer.
```

### Advanced Reasoning Strategy

```markdown
# Reasoning Strategy
1. Query Analysis: Break down the query until you're confident
   about what it's asking.
2. Context Analysis: Select a large set of potentially relevant
   documents. Optimize for recall.
3. Synthesis: Summarize which documents are most relevant and why.
```

### Common Error Sources

- Misunderstanding user intent
- Insufficient context analysis
- Incorrect reasoning steps

---

## 7. Long Context Techniques (1M tokens)

### Instruction Placement

With large context, the optimal approach is to place instructions at **both the beginning and the end**. If only placed once, **beginning is better than end**.

### Document Format

Ranked by effectiveness:

| Format | Example | Recommendation |
|--------|---------|----------------|
| XML | `<doc id='1' title='The Fox'>content</doc>` | ✅ Recommended |
| Lee Format | `ID: 1 \| TITLE: The Fox \| CONTENT: content` | ✅ Recommended |
| JSON Array | `[{"id": 1, "content": "..."}]` | ❌ Poor results |

### Controlling Context Dependency

- **Documents only**: `Only use the provided external context; if you don't know, say you don't know`
- **Mixed knowledge**: `Prioritize external context, but combine with your own knowledge when necessary`

---

## 8. Delimiter Selection Guide

| Delimiter | Best For |
|-----------|----------|
| **Markdown** | Default recommendation; use `#`/`##` for hierarchy, backticks for code |
| **XML** | Precisely wrapping content sections, especially nested structures with metadata |
| **JSON** | Clear structure but verbose, requires escaping, poor performance in long contexts |

**Key Principle**: Choose a delimiter different from your content format. If your document contains lots of XML, use Markdown.

---

## 9. Prompt Debugging Workflow

OpenAI's recommended iterative process:

```
1. Write # Instructions section with high-level points
       ↓
2. Specific behavior wrong? Add subsection (like # Sample Phrases)
       ↓
3. Need flow control? Add numbered step list
       ↓
4. Still not working?
   ├─ Check for conflicting instructions (end takes priority)
   ├─ Add examples demonstrating desired behavior
   └─ All-caps/bribes as last resort
```

---

## 10. Few-shot Examples

Provide diverse input-output example pairs in the developer message, clearly wrapped with XML tags:

```xml
<user_query>How do I declare a string variable to store a last name?</user_query>
<assistant_response>var last_name = "Smith";</assistant_response>
```

**Key Points**:
- Examples should cover different types of input scenarios
- Behavior patterns shown in examples must align with your rules

---

## 11. Reasoning Models vs GPT Models

A core distinction OpenAI emphasizes repeatedly:

| Dimension | GPT Models (gpt-4.1, gpt-5) | Reasoning Models (o1, o3, o4-mini) |
|-----------|---------------------------|------------------------------------|
| Instruction detail | More specific is better | High-level goals sufficient; too detailed interferes |
| Chain of Thought | Requires prompt induction | Built-in implicit CoT; **do not** add CoT instructions |
| Markdown output | Default output Markdown | Default **no** Markdown output; need `Formatting re-enabled` |
| RAG context volume | Can inject large amounts | Should be concise; only most relevant |
| Message roles | Supports system | Use `developer` instead of system |
| Cost/latency | Fast and cheap | Slow and expensive; for truly difficult tasks |

**Analogy**:
- **GPT Models = Junior Colleague**: Needs precise, detailed instructions
- **Reasoning Models = Senior Colleague**: Just give goals and constraints, let them figure it out

**Application**: If your LLM Gateway routes to different models, you should have two separate prompt templates.

---

## 12. GPT-5 New Prompting Strategies

### Frontend Engineering Prompts

GPT-5 can generate complete frontend applications from a single prompt. Recommended to add internal evaluation rubrics:

```markdown
Step 1: Create an evaluation rubric and refine it.
Step 2: Consider every element that defines a world-class solution,
        create a rubric with 5-7 categories. Keep this hidden.
Step 3: Apply the rubric to iterate until optimal.
Step 4: Aim for simplicity, avoid external dependencies.
```

### Agent Task Tracking

- Use TODO tools or checklists to track multi-step progress
- Provide brief preamble before important tool calls

---

## 13. Diff Format (Code Agent Specific)

GPT-4.1 has specialized training optimization for diff generation. OpenAI's open-source **V4A diff format** has these core features:

- **No line numbers**—uses context to locate code position
- Provides original code to be replaced + new code
- Uses clear delimiters to distinguish

**SEARCH/REPLACE format** and **pseudo-XML format** also work well.

**Application**: Build `apply_patch` tool with format specification in tool definitions.

---

## 14. Practical Examples

### Loom PM Agent System Prompt

```markdown
# Role and Objective
You are a project management Agent responsible for understanding user
requirements, generating Specs, and coordinating with Dev Agent.

# Instructions
- Never access code directly; use delegate_to_agent tool for technical research
- Must confirm user intent before generating Spec
- [other rules...]

# Workflow Steps
1. Analyze user request, break down into subtasks
2. For each subtask, determine if technical research is needed
3. If needed, call delegate_to_agent
4. Compile results, generate Spec draft
5. Confirm Spec with user

# Output Format
[Spec structure template]

# Examples
[Complete example from requirement to Spec]
```

### XAgent Twitter Search Agent

- Leverage tool definition best practices
- Write clear name/description for each MCP tool
- Place actual call examples in system prompt's Examples section

---

## Appendix: Quick Checklist

- [ ] Each Markdown heading is clear
- [ ] developer/user/assistant roles used correctly
- [ ] No conflicting instructions
- [ ] Three agentic keys included (persistence, tool use, planning)
- [ ] Tool definitions passed via API `tools` field
- [ ] Few-shot examples cover different scenarios
- [ ] Long context uses XML/Lee format
- [ ] Prompt strategy adjusted for model type (GPT/Reasoning)

---

*Last updated: 2024*
