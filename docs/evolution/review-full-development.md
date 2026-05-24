# Review 功能全景开发文档（v2 — 完全复制 Hermes 架构）

> 基于 Hermes `run_agent.py:3984-4250` 源码分析，完全复制其 Agent 模式架构。

## 一、架构差异：Hermes Agent 模式 vs 当前 JSON 模式

### 1.1 Hermes 的架构

```
对话结束
    │
    ▼
_evaluate_review_triggers() → 判断 review_memory / review_skills
    │
    ▼
_spawn_background_review(messages_snapshot, review_memory, review_skills)
    │
    ▼
创建 daemon 线程 "bg-review"
    │
    ▼
在 daemon 线程中：
    1. fork AIAgent（继承 model/provider/api_key/base_url/system_prompt）
    2. 设置工具白名单：只允许 memory + skill_manage 工具集
    3. 设置 _memory_write_origin = "background_review"
    4. 继承父 agent 的 _cached_system_prompt（复用 prefix cache）
    5. 调用 review_agent.run_conversation(
           user_message = prompt + "\n\nYou can only call memory and skill management tools...",
           conversation_history = messages_snapshot,
       )
    │
    ▼
Fork Agent 自主运行（最多 16 轮迭代）：
    - LLM 自己决定调用哪些工具（memory tool / skill_manage tool）
    - skill_manage 工具支持：create / edit / patch / delete / write_file / remove_file
    - Agent 可以先 skills_list 查看现有技能，再决定 patch 还是 create
    - Agent 可以 skill_view 查看技能详情，精确 patch
    │
    ▼
_summarize_background_review_actions() → 提取成功操作摘要
    │
    ▼
向用户展示："💾 Self-improvement review: Memory updated · Skill 'debug-rust' created"
```

**关键点：Hermes 不是"LLM 输出 JSON → 代码解析执行"，而是"LLM 作为 Agent 自主调用工具完成操作"。**

### 1.2 当前实现（需替换）

```
对话结束（手动 CLI）
    │
    ▼
extract_session_text() → 提取文本
    │
    ▼
build_review_prompt(text) → 构造 prompt
    │
    ▼
LLM 单次调用 → 输出 JSON
    │
    ▼
parse_review_response() → 解析 JSON
    │
    ▼
apply_memory_updates() / apply_skill_suggestions() → 代码执行写入
```

**问题：LLM 无法浏览技能库、无法精确 patch、无法多步操作。**

### 1.3 目标架构（完全复制 Hermes）

```
对话结束
    │
    ▼
evaluate_review_triggers() → 判断维度
    │
    ▼
spawn_background_review(session_id, review_memory, review_skills)
    │
    ▼
创建 daemon 线程
    │
    ▼
在 daemon 线程中：
    1. 从 SessionManager 加载 messages_snapshot
    2. build_review_client() → LlmClient（继承主 agent provider）
    3. 创建工具集：
       - memory_get / memory_set（读写 USER.md/PROJECT.md/FACTS.md）
       - skills_list / skills_view（查看现有技能）
       - skill_create / skill_edit / skill_patch / skill_write_file（操作技能）
    4. 将这些工具注册为 LlmClient 可调用的 function tools
    5. 构造 system prompt = 选择对应的 review prompt
    6. 以 messages_snapshot 为对话历史 + review prompt 为最新 user message
    7. 运行 agent 循环（最多 16 轮），每轮：
       a. LLM 返回 tool_calls
       b. 执行对应工具
       c. 将工具结果返回给 LLM
       d. 重复直到 LLM 不再调用工具
    │
    ▼
从 agent 循环的 tool 结果中提取成功操作
    │
    ▼
记录到 ReviewHistory + 向用户展示摘要
```

---

## 二、Hermes 源码逐段分析

### 2.1 三种 Review Prompt

#### `_MEMORY_REVIEW_PROMPT`（仅记忆）

```
Review the conversation above and consider saving to memory if appropriate.

Focus on:
1. Has the user revealed things about themselves — their persona, desires,
   preferences, or personal details worth remembering?
2. Has the user expressed expectations about how you should behave, their work
   style, or ways they want you to operate?

If something stands out, save it using the memory tool.
If nothing is worth saving, just say 'Nothing to save.' and stop.
```

#### `_SKILL_REVIEW_PROMPT`（仅技能）

```
Review the conversation above and update the skill library. Be ACTIVE — most
sessions produce at least one skill update, even if small. A pass that does
nothing is a missed learning opportunity, not a neutral outcome.

Target shape of the library: CLASS-LEVEL skills, each with a rich SKILL.md
and a `references/` directory for session-specific detail. Not a long flat
list of narrow one-session-one-skill entries.

Signals that warrant a skill update (any one is enough):
  • User corrected your style, tone, format, legibility, verbosity, or
    approach. Frustration is a FIRST-CLASS skill signal, not just a memory
    signal. 'stop doing X', 'don't format like this', 'I hate when you Y'
    — embed the lesson in the skill that governs that task so the next
    session starts fixed.
  • Non-trivial technique, fix, workaround, or debugging path emerged.
  • A skill that was loaded or consulted turned out wrong, missing, or
    outdated — patch it now.

Preference order for skills — pick the earliest that fits:
  1. UPDATE A CURRENTLY-LOADED SKILL. Check what skills were loaded via
     /skill-name or skill_view in the conversation. If one of them covers
     the learning, PATCH it first. It was in play; it's the right place.
  2. UPDATE AN EXISTING UMBRELLA (skills_list + skill_view to find the
     right one). Patch it.
  3. ADD A SUPPORT FILE under an existing umbrella via skill_manage
     action=write_file. Three kinds: `references/<topic>.md` for
     session-specific detail OR condensed knowledge banks (quoted research,
     API docs excerpts, domain notes) written concise and task-focused;
     `templates/<name>.<ext>` for starter files meant to be copied and
     modified; `scripts/<name>.<ext>` for statically re-runnable actions
     (verification, fixture generators, probes). Add a one-line pointer
     in SKILL.md so future agents find them.
  4. CREATE A NEW CLASS-LEVEL UMBRELLA when nothing exists. Name at the
     class level — NOT a PR number, error string, codename,
     library-alone name, or 'fix-X / debug-Y' session artifact. If the
     name only fits today's task, fall back to (1), (2), or (3).

If you notice overlapping existing skills, mention it — the background
curator handles consolidation.

Do NOT capture as skills (these become persistent self-imposed constraints
that bite you later when the environment changes):
  • Environment-dependent failures: missing binaries, fresh-install errors,
    post-migration path mismatches, 'command not found', unconfigured
    credentials, uninstalled packages.
  • Negative claims about tools or features ('browser tools do not work',
    'X tool is broken', 'cannot use Y from execute_code'). These harden
    into refusals the agent cites against itself for months after the
    actual problem was fixed.
  • Session-specific transient errors that resolved before the conversation
    ended. If retrying worked, the lesson is the retry pattern, not the
    original failure.
  • One-off task narratives. A user asking 'summarize today's market' or
    'analyze this PR' is not a class of work that warrants a skill.

If a tool failed because of setup state, capture the FIX (install command,
config step, env var to set) under an existing setup or troubleshooting
skill — never 'this tool does not work' as a standalone constraint.

Act on the skill dimension. If genuinely nothing stands out, say 'Nothing
to save.' and stop — but don't reach for that conclusion as a default.
```

#### `_COMBINED_REVIEW_PROMPT`（完整版）

```
Review the conversation above and update two things:

**Memory**: who the user is. Did the user reveal persona, desires,
preferences, personal details, or expectations about how you should behave?
Save facts about the user and durable preferences with the memory tool.

**Skills**: how to do this class of task. Be ACTIVE — most sessions produce
at least one skill update. A pass that does nothing is a missed learning
opportunity, not a neutral outcome.

Target shape of the skill library: CLASS-LEVEL skills with a rich SKILL.md
and a `references/` directory for session-specific detail. Not a long flat
list of narrow one-session-one-skill entries.

Signals that warrant a skill update (any one is enough):
  • User corrected your style, tone, format, legibility, verbosity, or
    approach. Frustration is a FIRST-CLASS skill signal, not just a memory
    signal. 'stop doing X', 'don't format like this', 'I hate when you Y'
    — embed the lesson in the skill that governs that task so the next
    session starts fixed.
  • Non-trivial technique, fix, workaround, or debugging path emerged.
  • A skill that was loaded or consulted turned out wrong, missing, or
    outdated — patch it now.

Preference order for skills — pick the earliest that fits:
  1. UPDATE A CURRENTLY-LOADED SKILL. Check what skills were loaded via
     /skill-name or skill_view in the conversation. If one of them covers
     the learning, PATCH it first. It was in play; it's the right place.
  2. UPDATE AN EXISTING UMBRELLA (skills_list + skill_view to find the
     right one). Patch it.
  3. ADD A SUPPORT FILE under an existing umbrella via skill_manage
     action=write_file. Three kinds: `references/<topic>.md` for
     session-specific detail OR condensed knowledge banks; `templates/
     <name>.<ext>` for starter files; `scripts/<name>.<ext>` for
     statically re-runnable actions. Add a one-line pointer in SKILL.md
     so future agents find them.
  4. CREATE A NEW CLASS-LEVEL UMBRELLA when nothing exists. Name at the
     class level — NOT a PR number, error string, codename,
     library-alone name, or 'fix-X / debug-Y' session artifact.

User-preference embedding: when the user complains about how you handled
a task, update the skill that governs that task — memory alone isn't enough.
Memory says 'who the user is and what the current situation and state of
your operations are'; skills say 'how to do this class of task for this
user'. Both should carry user-preference lessons when relevant.

If you notice overlapping existing skills, mention it — the background
curator handles consolidation.

Do NOT capture as skills:
  • Environment-dependent failures: missing binaries, fresh-install errors,
    post-migration path mismatches, 'command not found', unconfigured
    credentials, uninstalled packages.
  • Negative claims about tools or features ('browser tools do not work',
    'X tool is broken'). These harden into refusals the agent cites against
    itself for months after the actual problem was fixed.
  • Session-specific transient errors that resolved before the conversation
    ended. If retrying worked, the lesson is the retry pattern, not the
    original failure.
  • One-off task narratives. A user asking 'summarize today's market' or
    'analyze this PR' is not a class of work that warrants a skill.

If a tool failed because of setup state, capture the FIX (install command,
config step, env var to set) under an existing setup or troubleshooting
skill — never 'this tool does not work' as a standalone constraint.

Act on whichever of the two dimensions has real signal. If genuinely
nothing stands out on either, say 'Nothing to save.' and stop — but don't
reach for that conclusion as a default.
```

### 2.2 Agent Fork 的关键细节

```python
# 1. 继承父 agent 的运行时配置
review_agent = AIAgent(
    model=self.model,                          # 同一个模型
    max_iterations=16,                          # 最多 16 轮工具调用
    quiet_mode=True,                            # 静默模式
    provider=self.provider,                     # 同一个 provider
    api_key=...,                                # 同一个 api_key
    base_url=...,                               # 同一个 base_url
    parent_session_id=self.session_id,          # 关联父 session
)

# 2. 标记来源
review_agent._memory_write_origin = "background_review"
review_agent._memory_write_context = "background_review"

# 3. 共享 memory/skill store
review_agent._memory_store = self._memory_store
review_agent._memory_enabled = self._memory_enabled

# 4. 继承 system prompt（复用 prefix cache）
review_agent._cached_system_prompt = self._cached_system_prompt
review_agent.session_start = self.session_start
review_agent.session_id = self.session_id

# 5. 禁用自动 review 递归
review_agent._memory_nudge_interval = 0
review_agent._skill_nudge_interval = 0

# 6. 抑制输出
review_agent.suppress_status_output = True

# 7. 工具白名单
review_whitelist = {
    t["function"]["name"]
    for t in get_tool_definitions(
        enabled_toolsets=["memory", "skills"],  # 只有这两个工具集
        quiet_mode=True,
    )
}
set_thread_tool_whitelist(review_whitelist)

# 8. 运行 agent 对话
review_agent.run_conversation(
    user_message=(
        prompt
        + "\n\nYou can only call memory and skill management tools. "
        + "Other tools will be denied at runtime — do not attempt them."
    ),
    conversation_history=messages_snapshot,     # 完整对话历史
)

# 9. 安全控制：自动拒绝危险命令
def _bg_review_auto_deny(command, description, **kwargs):
    return "deny"
_set_approval_callback(_bg_review_auto_deny)
```

### 2.3 skill_manage 工具的 Action

Hermes 的 `skill_manage` 工具支持 6 种 action：

| Action | 说明 | 参数 |
|--------|------|------|
| `create` | 创建新技能（SKILL.md + 目录结构） | name, description, triggers, body |
| `edit` | 全量重写 SKILL.md | name, content |
| `patch` | 精确 find-and-replace | name, old_string, new_string |
| `delete` | 删除技能 | name |
| `write_file` | 添加/覆写支撑文件 | name, path, content |
| `remove_file` | 删除支撑文件 | name, path |

### 2.4 memory 工具

Hermes 的 memory 工具：

| 工具 | 说明 |
|------|------|
| `memory` | 读写 memory 文件（add/replace/get 等操作） |

### 2.5 结果摘要

```python
def _summarize_background_review_actions(review_messages, prior_snapshot):
    """从 review agent 的 tool 结果中提取成功操作"""
    # 跳过 prior_snapshot 中已有的 tool messages（避免重复计数）
    # 只收集新产生的 tool 调用
    # 过滤条件：success=true + 包含 created/updated/added/removed/replaced
    # 返回 ["Memory updated", "Skill 'debug-rust' created"] 等
```

向用户展示：
```
💾 Self-improvement review: Memory updated · Skill 'debug-rust-errors' created
```

---

## 三、Loom 适配方案

### 3.1 核心挑战

Hermes 的 Agent 模式依赖：
1. **LLM function calling**（tool_calls）— LLM 主动调用工具
2. **多轮 Agent 循环**（最多 16 轮）— LLM 可以先 list 再 view 再 patch
3. **工具白名单机制** — 非 memory/skill 工具被拒绝

Loom 当前有 `loom::llm::LlmClient` 支持 tool calling 吗？

**需要确认**：检查 `LlmClient::invoke()` 是否返回 `tool_calls`，以及 `ChatOpenAI` 是否支持 function calling。

### 3.2 方案选择

**方案 A：使用 LlmClient 的 tool calling 能力**（如果已支持）
- 定义 review 工具为 function definitions
- 构造 agent 循环：invoke → 处理 tool_calls → 返回结果 → 再次 invoke
- 最接近 Hermes

**方案 B：使用 invoke_agent + Agent Profile**
- 创建 `.loom/agents/reviewer/profile.yaml`
- 定义工具白名单为 memory + skill 操作
- 通过 `invoke_agent(agent="reviewer", task=review_prompt, async=true)` 触发
- 复用 Loom 已有的 agent 框架

**方案 C：保持 JSON 模式但增强能力**
- 不用 tool calling，LLM 输出 JSON
- JSON 中增加 "browse_skills" 等虚拟动作
- 代码先执行浏览，再二次调用 LLM

### 3.3 推荐方案：方案 B（invoke_agent）

理由：
1. Loom 已有 `invoke_agent` 基础设施
2. Agent profile 支持工具限制
3. 不需要自己实现 agent 循环
4. 与 Loom 的 agent 架构一致

---

## 四、实施计划

### Phase 0: 确认 LlmClient tool calling 能力

**任务**：
1. 确认 `LlmClient::invoke()` 返回的 `LlmResponse` 是否包含 `tool_calls` 字段
2. 确认 `ChatOpenAI` / `ChatOpenAICompat` 是否支持发送 `tools` 参数
3. 如果支持 → 可选方案 A
4. 如果不支持 → 方案 B（invoke_agent）

**验收**：
- [ ] 明确 LlmClient 的 tool calling 支持状态
- [ ] 选择最终方案

### Phase 1: Review Prompt 对齐

**任务**：
1. 将 Hermes 的三个 prompt（`_MEMORY_REVIEW_PROMPT`、`_SKILL_REVIEW_PROMPT`、`_COMBINED_REVIEW_PROMPT`）原样复制到 Rust 常量
2. 不做任何修改，包括英文原文
3. 将 prompt 文本替换当前 `build_review_prompt()` 的输出

**实现**：

```rust
// cli/src/run/review_prompts.rs — 新文件

pub const MEMORY_REVIEW_PROMPT: &str = "\
Review the conversation above and consider saving to memory if appropriate.\n\n\
Focus on:\n\
1. Has the user revealed things about themselves — their persona, desires, \
preferences, or personal details worth remembering?\n\
2. Has the user expressed expectations about how you should behave, their work \
style, or ways they want you to operate?\n\n\
If something stands out, save it using the memory tool.\n\
If nothing is worth saving, just say 'Nothing to save.' and stop.";

pub const SKILL_REVIEW_PROMPT: &str = "\
Review the conversation above and update the skill library. Be ACTIVE — most \
sessions produce at least one skill update, even if small. A pass that does \
nothing is a missed learning opportunity, not a neutral outcome.\n\n\
// ... 完整复制 Hermes _SKILL_REVIEW_PROMPT
";

pub const COMBINED_REVIEW_PROMPT: &str = "\
Review the conversation above and update two things:\n\n\
// ... 完整复制 Hermes _COMBINED_REVIEW_PROMPT
";
```

**验收**：
- [ ] 三个 prompt 常量与 Hermes 源码完全一致
- [ ] 当前 JSON 模式的 `ReviewAgent` 改为使用 `COMBINED_REVIEW_PROMPT`
- [ ] `cargo build` 通过

### Phase 2: Review 工具定义

**任务**：
定义 Hermes review agent 可用的工具集，等价于 Hermes 的 `memory` + `skills` 工具集。

**工具清单**：

```rust
// 等价于 Hermes 的 memory 工具
ReviewTool::MemoryGet { file: String }     // 读取 USER/PROJECT/FACTS.md
ReviewTool::MemorySet { file: String, action: String, content: String }  // 写入记忆

// 等价于 Hermes 的 skill_manage 工具
ReviewTool::SkillsList                     // 列出所有技能
ReviewTool::SkillView { name: String }     // 查看技能详情
ReviewTool::SkillCreate { name: String, description: String, triggers: Vec<String>, body: String }
ReviewTool::SkillEdit { name: String, content: String }    // 全量重写
ReviewTool::SkillPatch { name: String, old_string: String, new_string: String }  // 精确替换
ReviewTool::SkillWriteFile { name: String, path: String, content: String }  // 添加支撑文件
ReviewTool::SkillDelete { name: String }
ReviewTool::SkillRemoveFile { name: String, path: String }
```

**验收**：
- [ ] 所有 Hermes review 工具都有等价 Rust 实现
- [ ] 每个工具的 function definition JSON 与 Hermes 格式兼容
- [ ] 工具执行逻辑连接到 MemoryStore / SkillRegistry

### Phase 3: Agent 循环实现

**任务**：
实现等价于 Hermes `review_agent.run_conversation()` 的 Agent 循环。

**核心逻辑**：

```rust
pub async fn run_review_agent_loop(
    llm: &dyn LlmClient,
    tools: &[ToolDefinition],
    messages_snapshot: &[Message],
    review_prompt: &str,
    max_iterations: usize,
) -> Result<Vec<ReviewAction>, String> {
    let mut messages: Vec<Message> = messages_snapshot.to_vec();
    messages.push(Message::user(
        format!("{}\n\nYou can only call memory and skill management tools. \
                 Other tools will be denied at runtime — do not attempt them.",
                 review_prompt)
    ));

    let mut actions = Vec::new();

    for _ in 0..max_iterations {
        let response = llm.invoke_with_tools(&messages, tools).await
            .map_err(|e| format!("LLM call failed: {}", e))?;

        if response.tool_calls.is_empty() {
            // LLM 不再调用工具，循环结束
            break;
        }

        for tool_call in &response.tool_calls {
            let result = execute_review_tool(tool_call)?;
            if let Some(action) = result.action_summary() {
                actions.push(action);
            }
            // 将工具结果添加到 messages
            messages.push(Message::tool_result(tool_call.id, result.to_json()));
        }

        messages.push(response.to_assistant_message());
    }

    Ok(actions)
}
```

**验收**：
- [ ] Agent 循环最多 16 轮
- [ ] 每轮正确处理 tool_calls
- [ ] 工具结果正确返回给 LLM
- [ ] 循环在 LLM 不再调用工具时终止

### Phase 4: 自动触发 + 触发条件评估

**任务**：
实现等价于 Hermes `_evaluate_review_triggers()` 的触发条件评估。

**Hermes 的触发逻辑**（`run_agent.py`）：
- 每轮对话结束后评估是否需要 review
- 检查 `review_memory` 和 `review_skills` 两个布尔值
- 根据结果选择 prompt 类型（memory-only / skill-only / combined）
- 调用 `_spawn_background_review(messages_snapshot, review_memory, review_skills)`

**实现**：

```rust
pub fn spawn_background_review(
    session_id: String,
    messages_snapshot: Vec<Message>,
    review_memory: bool,
    review_skills: bool,
) -> std::thread::JoinHandle<()> {
    let prompt = match (review_memory, review_skills) {
        (true, true) => COMBINED_REVIEW_PROMPT,
        (true, false) => MEMORY_REVIEW_PROMPT,
        (false, true) => SKILL_REVIEW_PROMPT,
        (false, false) => return, // 不触发
    };

    std::thread::Builder::new()
        .name("bg-review".to_string())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all().build().unwrap();
            rt.block_on(async {
                // 1. build_review_client()
                // 2. 创建 ReviewToolExecutor
                // 3. 运行 agent 循环
                // 4. 记录到 ReviewHistory
                // 5. 向用户展示摘要
            });
        })
}
```

**验收**：
- [ ] 每轮对话结束后自动触发
- [ ] daemon 线程不阻塞主对话
- [ ] 触发条件正确选择 prompt 类型
- [ ] ReviewHistory 记录 trigger: "auto"

### Phase 5: 结果摘要 + 用户展示

**任务**：
实现等价于 Hermes `_summarize_background_review_actions()` 的结果摘要。

**逻辑**（完全复制 Hermes）：
1. 从 agent 循环的 tool 结果中筛选成功的操作
2. 跳过 messages_snapshot 中已有的 tool messages（避免重复计数）
3. 过滤条件：`success=true` + 包含 `created/updated/added/removed/replaced`
4. 格式化为一行摘要

**输出示例**：
```
💾 Self-improvement review: Memory updated · Skill 'debug-rust-errors' created
```

**验收**：
- [ ] 正确去重已有 tool messages
- [ ] 只摘要成功的操作
- [ ] 格式化为 Hermes 风格的单行摘要

### Phase 6: 安全控制

**任务**：
复制 Hermes 的安全机制。

1. **工具白名单**：非 memory/skill 工具被拒绝
2. **危险命令自动拒绝**：`_bg_review_auto_deny()` — 自动 deny 所有危险命令
3. **agent-created 标记**：`_memory_write_origin = "background_review"`
4. **guard_agent_created**：可选的安全扫描
5. **输出抑制**：`suppress_status_output = True`
6. **prefix cache 复用**：继承父 agent 的 system prompt

**验收**：
- [ ] 非 memory/skill 工具调用被拒绝
- [ ] 危险命令自动 deny
- [ ] review 创建的内容标记为 agent-created
- [ ] review 过程无用户可见输出

### Phase 7: 配置集成

**任务**：
从 `.loom/config.yaml` 读取 review/skills/memory 配置。

```yaml
review:
  enabled: true
  max_session_chars: 12000
  max_iterations: 16
  model: null

memory:
  enabled: true
  max_chars: 8000

skills:
  auto_create: true
  guard_agent_created: false
```

**验收**：
- [ ] `review.enabled: false` 不触发
- [ ] `review.model` 覆盖默认
- [ ] `review.max_iterations` 限制循环次数

---

## 五、文件变更总览

| 文件 | Phase | 操作 | 说明 |
|------|-------|------|------|
| `cli/src/run/review_prompts.rs` | P1 | **新增** | Hermes 三个 prompt 原文常量 |
| `cli/src/run/review_tools.rs` | P2 | **新增** | Review 工具定义 + 执行器 |
| `cli/src/run/review_agent_loop.rs` | P3 | **新增** | Agent 循环（tool calling） |
| `cli/src/run/background_review.rs` | P4 | **新增** | 自动触发 + daemon 线程 |
| `cli/src/run/review.rs` | P1 | **重写** | 删除 JSON 模式，改用 Agent 模式 |
| `cli/src/run/evolution_config.rs` | P7 | **新增** | 配置解析 |
| `cli/src/run/skill_registry.rs` | P2 | **修改** | 新增 list/view/patch/write_file/remove_file |
| `cli/src/run/memory.rs` | P2 | **修改** | 新增 get/set 工具方法 |
| `cli/src/review_cmd.rs` | P1 | **修改** | 使用新 prompt + Agent 模式 |
| `cli/src/review_history.rs` | P4 | **修改** | 新增 action 记录字段 |
| `cli/src/repl.rs` | P4 | **修改** | 对话结束钩子 |

---

## 六、Hermes 遗漏清单（逐项核对）

逐项对照 `run_agent.py:3984-4250` 源码，确认文档覆盖所有细节：

| # | Hermes 源码细节 | 文档是否覆盖 | 位置 |
|---|----------------|-------------|------|
| 1 | 三种 prompt 原文（MEMORY/SKILL/COMBINED） | ✅ | §2.1 |
| 2 | `_evaluate_review_triggers()` 触发条件评估 | ✅ | §Phase 4 |
| 3 | daemon 线程 `threading.Thread(daemon=True, name="bg-review")` | ✅ | §Phase 4 |
| 4 | fork AIAgent 继承 model/provider/api_key/base_url | ✅ | §2.2 |
| 5 | `max_iterations=16` | ✅ | §2.2 + Phase 3 |
| 6 | `quiet_mode=True` | ✅ | §Phase 6 |
| 7 | 工具白名单 `enabled_toolsets=["memory", "skills"]` | ✅ | §Phase 6 |
| 8 | 白名单拒绝消息格式 | ✅ | §2.2 |
| 9 | `_memory_write_origin = "background_review"` | ✅ | §Phase 6 |
| 10 | `_memory_write_context = "background_review"` | ✅ | §Phase 6 |
| 11 | 共享 `_memory_store` | ✅ | §2.2 |
| 12 | 继承 `_cached_system_prompt`（prefix cache） | ✅ | §Phase 6 |
| 13 | `session_start`/`session_id` 继承（保证 prefix cache 一致） | ✅ | §2.2 |
| 14 | `_memory_nudge_interval = 0`（防递归 review） | ✅ | §2.2 |
| 15 | `_skill_nudge_interval = 0`（防递归 review） | ✅ | §2.2 |
| 16 | `suppress_status_output = True` | ✅ | §Phase 6 |
| 17 | `parent_session_id=self.session_id` | ✅ | §2.2 |
| 18 | `run_conversation(user_message, conversation_history)` 模式 | ✅ | §Phase 3 |
| 19 | prompt 附加 "You can only call memory and skill management tools..." | ✅ | §Phase 3 |
| 20 | `_bg_review_auto_deny` 危险命令自动拒绝 | ✅ | §Phase 6 |
| 21 | `_set_approval_callback` 机制 | ✅ | §Phase 6 |
| 22 | `clear_thread_tool_whitelist()` finally 清理 | ✅ | §Phase 6 |
| 23 | `shutdown_memory_provider()` 清理 | ✅ | §Phase 6 |
| 24 | `review_agent.close()` 清理 | ✅ | §Phase 6 |
| 25 | `contextlib.redirect_stdout/stderr(devnull)` 输出抑制 | ✅ | §Phase 6 |
| 26 | `_summarize_background_review_actions()` 去重逻辑 | ✅ | §Phase 5 |
| 27 | 按 tool_call_id 去重 + content fallback | ✅ | §Phase 5 |
| 28 | 过滤条件：success=true + created/updated/added/removed | ✅ | §Phase 5 |
| 29 | `_safe_print` 输出摘要 | ✅ | §Phase 5 |
| 30 | `background_review_callback` 回调通知 | ✅ | §Phase 5 |
| 31 | codex_app_server 降级为 codex_responses | ⚠️ Loom 无 codex，可忽略 |
| 32 | credential_pool 继承 | ⚠️ Loom 无此概念，可忽略 |
| 33 | `guard_agent_created` 安全扫描 | ✅ | §Phase 6 |
| 34 | skill_manage 6 种 action | ✅ | §2.3 |
| 35 | memory 工具 | ✅ | §2.4 |
| 36 | `_emit_auxiliary_failure` 错误通知 | ❌ **遗漏** | 需补充 |
| 37 | `_current_main_runtime()` 获取运行时 | ⚠️ 映射为 build_review_client() |
| 38 | api_mode 降级逻辑 | ⚠️ Loom 无此概念 |
| 39 | review agent 的 `_session_messages` 收集 | ❌ **遗漏** | 需补充 |
| 40 | `_memory_enabled`/`_user_profile_enabled` 继承 | ⚠️ 简化为配置读取 |

**遗漏补充：**

### 补充 1: `_emit_auxiliary_failure` 错误通知

Hermes 在 background review 失败时，通过 `_emit_auxiliary_failure("background review", e)` 通知上层。
Loom 应通过 `tracing::warn!` 记录错误日志。

### 补充 2: review agent 的 `_session_messages` 收集

Hermes 在 `_summarize_background_review_actions` 中读取 `review_agent._session_messages` 获取 fork agent 产生的所有消息。
Loom 的 agent 循环应返回 `Vec<ToolResult>` 供摘要提取。

---

## 七、依赖确认清单

在开始开发前，必须确认：

- [ ] `loom::llm::LlmClient` 是否支持 `invoke_with_tools(messages, tool_definitions)`？
- [ ] `LlmResponse.tool_calls` 字段结构是什么？
- [ ] `ChatOpenAI` / `ChatOpenAICompat` 是否向 API 发送 `tools` 参数？
- [ ] 如果不支持 tool calling，`invoke_agent` 能否作为替代方案？
- [ ] `loom::message::Message` 是否支持 `tool_result` variant？
- [ ] 现有的 `LlmClient::invoke()` 是否已处理 tool calling 循环？

---

## 八、实施预估

| Phase | 预估 | 前置 |
|-------|------|------|
| Phase 0: 确认 tool calling 能力 | 0.5 天 | 无 |
| Phase 1: Prompt 对齐 | 0.5 天 | 无 |
| Phase 2: Review 工具定义 | 2 天 | Phase 0 |
| Phase 3: Agent 循环 | 2 天 | Phase 0+2 |
| Phase 4: 自动触发 | 1 天 | Phase 3 |
| Phase 5: 结果摘要 | 0.5 天 | Phase 3 |
| Phase 6: 安全控制 | 1 天 | Phase 3 |
| Phase 7: 配置集成 | 0.5 天 | Phase 4 |
| **合计** | **8 天** | |

最小可行路径：Phase 0 + Phase 1 + Phase 2 + Phase 3 = 5 天跑通 Agent 模式审查。

---

## 九、与当前实现的切换策略

当前 JSON 模式代码保留为 fallback：

```rust
pub enum ReviewMode {
    Agent,  // Hermes 模式（tool calling）
    Json,   // 当前模式（JSON 输出）
}

// 配置切换
// review:
//   mode: agent  # 或 "json"
```

Phase 3 完成前，手动 CLI 命令继续使用 JSON 模式。
Phase 3 完成后，自动触发使用 Agent 模式。
Phase 7 后，通过配置切换默认模式。
