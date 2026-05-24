# Hermes Agent 进化能力分析 → Loom 适配方案

## 一、Hermes Agent 核心进化能力（6 大机制）

### 1. 闭环学习循环 (Closed Learning Loop)
- **机制**: 每轮对话后，自动 fork 一个后台 Agent 线程，回放对话快照，让 Agent 自问"是否值得保存记忆或技能"
- **关键文件**: `agent/background_review.py`
- **三个审查维度**:
  - `_MEMORY_REVIEW_PROMPT`: 保存用户偏好、个人信息、行为期望
  - `_SKILL_REVIEW_PROMPT`: 更新/创建/修补技能文档
  - `_COMBINED_REVIEW_PROMPT`: 同时处理两者
- **触发条件**: 每轮对话结束后（在 `AIAgent.run_conversation` 中调用 `spawn_background_review`）
- **安全约束**: 后台 Agent 只能用 memory 和 skill_manage 工具，其他工具被拒绝

### 2. 三层持久记忆系统
- **机制**: MemoryManager 统一管理，仅允许一个外部 Provider
  - **Episodic Memory** (会话记忆): 当前对话上下文
  - **Skill Memory** (技能记忆): SKILL.md + references/ + templates/ + scripts/ 目录结构
  - **Volatile Memory** (易变记忆): 内存快照，每轮注入 system prompt
- **关键文件**: `agent/memory_manager.py`, `tools/memory_tool.py`
- **存储位置**: `~/.hermes/skills/` 目录
- **Provider 插件**: 支持 Honcho, Holographic, Mem0, SuperMemory, RetainDB, Byterover 等 7+ 后端
- **核心原则**: 记忆是声明式事实（"用户偏好简洁回答"），不是指令式（"总是简洁回答"）

### 3. 技能自动创建与自我改进 (Skill Auto-Creation)
- **机制**: 每轮对话结束后，`spawn_background_review` 启动 daemon 线程，fork 一个 agent 回放对话快照，用 `_COMBINED_REVIEW_PROMPT` 决定是否创建/更新技能（无需用户干预）
- **关键文件**: `agent/background_review.py`, `tools/skill_manager_tool.py`, `tools/skill_usage.py`, `tools/skill_provenance.py`
- **技能生命周期**: `active → stale → archived`（由 Curator 管理，永不删除）
- **技能结构**:
  ```
  ~/.hermes/skills/my-skill/
  ├── SKILL.md          # YAML frontmatter + Markdown 正文
  ├── references/       # 会话细节、错误日志、API 文档摘录
  ├── templates/        # 可复用的模板文件
  ├── scripts/          # 可直接执行的脚本
  └── assets/           # 其他资源
  ```
- **技能操作**: `skill_manage(action=create|edit|patch|delete|write_file|remove_file)`
- **Review 优先级链**（优先匹配前项，最后才创建新技能）:
  1. PATCH 当前会话已加载的技能
  2. PATCH 已有的 class-level umbrella 技能
  3. 在已有 umbrella 下添加 support file（`references/` / `templates/` / `scripts/`）
  4. CREATE 新的 class-level umbrella 技能（仅当无任何现有技能覆盖时）
- **agent-created 标记**: 后台 review fork 创建的技能通过 `mark_agent_created()` 标记为 `agent-created`（`skill_manager_tool.py:773-782`，通过 `is_background_review()` 判断）
- **安全扫描**: agent-created 技能默认不扫描（`skills.guard_agent_created` 默认 false），开启后经 `skills_guard.py` 安全检查
- **Curator 联动**: `agent/curator.py` 独立运行 consolidation pass，只操作 agent-created 技能，合并狭窄技能为 umbrella，不碰 bundled/hub-installed/pinned 技能
- **信号检测**: review prompt 明确列出触发信号——用户纠正风格/流程、非平凡技巧/修复/workaround、已加载技能过时/缺失
- **反模式保护**: 不保存环境依赖的临时故障、工具负面断言、瞬时错误、一次性任务叙事

### 4. Curator 后台技能维护器
- **机制**: Agent 空闲时自动运行的技能审查系统
- **关键文件**: `agent/curator.py`
- **职责**:
  - 自动转换技能生命周期状态（基于使用时间戳）
  - Pin / Archive / Consolidate / Patch 技能
  - 从不自动删除（只 archive，可恢复）
  - Pinned 技能跳过所有自动转换
- **触发**: Agent 空闲 + 上次运行超过 7 天时自动触发
- **使用辅助模型**: 不影响主会话的 prompt cache

### 5. DSPy + GEPA 进化优化 (Self-Evolution)
- **机制**: 用遗传-帕累托提示词进化自动优化技能文件
- **关键文件**: `hermes-agent-self-evolution/evolution/skills/evolve_skill.py`
- **进化流程**:
  1. 加载目标技能 (SKILL.md)
  2. 构建评估数据集（synthetic/golden/sessiondb 三种来源）
  3. 将技能包装为 DSPy Module（skill_text 是可优化参数）
  4. 用 `dspy.GEPA(metric=skill_fitness_metric)` 运行进化
  5. 在 holdout 集上对比 baseline vs evolved
  6. 通过约束验证后保存
- **适应度评分** (`evolution/core/fitness.py`):
  - correctness (0.5 权重)
  - procedure_following (0.3 权重)
  - conciseness (0.2 权重)
  - length_penalty (防止膨胀)
- **约束系统** (`evolution/core/constraints.py`):
  - 技能大小 ≤ 15KB
  - 增长幅度 ≤ 20%
  - YAML frontmatter 完整性
  - 可选: 全量 pytest 通过
- **5 阶段路线图**:
  1. ✅ Skill 文件进化
  2. 🔲 工具描述进化
  3. 🔲 System Prompt 进化
  4. 🔲 工具实现代码进化（Darwinian Evolver）
  5. 🔲 持续自动改进循环

### 6. System Prompt 三层组装
- **机制**: 每次会话只组装一次，不中途变异（保持 prompt cache）
- **关键文件**: `agent/system_prompt.py`, `agent/prompt_builder.py`
- **三层**:
  - `stable`: 身份(SOUL.md)、工具引导、技能索引、环境提示、平台提示
  - `context`: AGENTS.md / .cursorrules / .hermes.md + 用户 system_message
  - `volatile`: 记忆快照、USER.md 用户画像、外部记忆 Provider、时间戳

---

## 二、Loom 现有能力对照

| 能力 | Hermes | Loom 现状 |
|------|--------|-----------|
| 持久记忆 | ✅ MemoryManager + 多 Provider | ⚠️ 仅有 `/root/.loom/data` 目录，无结构化记忆 |
| 技能系统 | ✅ SKILL.md 完整体系 | ✅ `.loom/skills/` 目录已存在 |
| 后台审查 | ✅ background_review.py | ❌ 无 |
| 技能自动创建 | ✅ skill_manage 工具 | ⚠️ skill 可手动加载，但无自动创建 |
| 进化优化 | ✅ DSPy + GEPA | ❌ 无 |
| System Prompt 组装 | ✅ 三层组装 | ⚠️ 有 system prompt 但无动态记忆注入 |
| Curator 维护 | ✅ 后台 Curator | ❌ 无 |
| 会话搜索 | ✅ FTS5 session search | ❌ 无 |
| 用户建模 | ✅ Honcho 等 Provider | ❌ 无 |

---

## 三、Loom 适配方案（按优先级排序）

### Phase 1: 持久记忆层 (Memory Layer) — 基础设施

**目标**: 让 Loom 跨会话记住用户偏好和项目上下文

**实现**:
```
/root/.loom/data/memory/
├── USER.md          # 用户画像（声明式事实）
├── PROJECT.md       # 当前项目上下文
└── FACTS.md         # 通用持久事实
```

**方案**:
1. 在每次会话结束时，由 Loom 自行执行"记忆审查"——将关键事实写入上述文件
2. 每次会话开始时，自动读取这些文件注入 system prompt
3. 用 `.loom/agents/` 下的 profile 文件作为 system prompt 模板，支持 `{{memory}}` 占位符

**Loom 原生实现方式**: 利用 `.loom/agents/` 的 YAML profile + CLAUDE.md 中的自定义指令

### Phase 2: 技能自动创建 (Auto Skill Creation)

**目标**: Agent 完成复杂任务后自动生成可复用技能

**实现**:
```
/root/.loom/data/skills/
├── auto/
│   ├── debug-rust-errors/
│   │   ├── SKILL.md
│   │   └── references/
│   ├── deploy-vercel/
│   │   └── SKILL.md
│   └── ...
```

**方案**:
1. 在 `.loom/agents/` 的默认 profile 中加入指令："完成 5+ 步骤的复杂任务后，用 write_file 将解决方案保存为技能文件到 `.loom/skills/auto/<skill-name>/SKILL.md`"
2. 技能格式采用 Hermes 兼容的 YAML frontmatter + Markdown
3. 利用 Loom 已有的 `skill` 工具加载机制

**最小改动**: 只需在 system prompt/agent profile 中添加自省指令

### Phase 3: 后台审查循环 (Background Review Loop)

**目标**: 对话结束后自动回顾并更新记忆和技能

**方案**:
1. 利用 Loom 的 `invoke_agent` 机制——主对话结束后，异步启动一个 review sub-agent
2. Review Agent 的 system prompt 参考 Hermes 的 `_COMBINED_REVIEW_PROMPT`
3. Review Agent 只能使用 `read`, `write_file`, `edit` 工具（白名单限制）
4. 输出审查结果到用户可查看的位置

**Loom 实现方式**:
```yaml
# .loom/agents/reviewer/profile.yaml
name: background-reviewer
tools: [read, write_file, edit, glob, grep]
system_prompt: |
  Review the conversation and decide if any memory or skills should be updated...
```

### Phase 4: 技能进化优化 (Skill Evolution)

**目标**: 用 DSPy + GEPA 自动优化 Loom 的技能文件

**方案**:
1. 安装 `hermes-agent-self-evolution` 作为独立工具
2. 将 Loom 的 `.loom/skills/` 目录适配为 Hermes 兼容格式
3. 定期（如每周）对高频使用的技能运行进化优化
4. 将优化后的技能文件写回 `.loom/skills/`

**技术栈**: `pip install dspy gepa-ai`, 配置 OpenRouter/OpenAI API key

**运行命令**:
```bash
python -m evolution.skills.evolve_skill \
  --skill debug-rust-errors \
  --iterations 10 \
  --eval-source synthetic \
  --hermes-repo /root/.loom
```

### Phase 5: System Prompt 动态组装 (Dynamic Prompt Assembly)

**目标**: 参考 Hermes 三层组装，让 Loom 的 system prompt 包含记忆和技能上下文

**方案**:
1. `stable`: Loom agent profile 本身（已有）
2. `context`: 读取项目根目录的 `CLAUDE.md`, `AGENTS.md`, `.loom/agents/` 配置
3. `volatile`: 注入 `/root/.loom/data/memory/` 中的用户画像 + 活跃技能摘要

**Loom 实现方式**: 在 `.loom/agents/default/` 的 profile 中引用记忆文件

### Phase 6: Curator 维护器 (Skill Curator)

**目标**: 自动维护技能库的健康度

**方案**:
1. 创建 `.loom/agents/curator` profile
2. 利用 Loom 的 `invoke_agent` 异步运行
3. Curator 职责:
   - 标记 30 天未使用的技能为 stale
   - 合并重叠技能
   - 归档 90 天未使用的技能（不删除）
   - 生成维护报告

### Phase 7: 会话搜索 (Session Search)

**目标**: 跨会话搜索历史对话

**方案**:
1. 将 Loom 的对话历史存储为可搜索格式（SQLite + FTS5 或 Markdown 文件）
2. 创建 `session_search` skill，让 Agent 可以搜索历史
3. 利用 `.loom/data/sessions/` 目录存储会话记录

---

## 四、最小可行方案（MVP: 立即可做）

**只需 2 个文件，0 代码改动**:

### 1. 创建用户记忆文件
```bash
mkdir -p /root/.loom/data/memory
touch /root/.loom/data/memory/USER.md
```

### 2. 在 CLAUDE.md 或 .loom/agents/default/profile 中加入指令:

```
## 自我进化协议

### 记忆持久化
每次会话结束前，检查是否需要更新以下文件：
- /root/.loom/data/memory/USER.md — 用户偏好、个人信息
- /root/.loom/data/memory/PROJECT.md — 项目上下文
用声明式事实（"用户偏好简洁"），不要指令式（"总是简洁"）。

### 技能自动创建
完成 5+ 步骤的复杂任务后：
1. 在 /root/.loom/data/skills/auto/<skill-name>/ 创建 SKILL.md
2. 格式：YAML frontmatter (name, description, triggers) + Markdown 步骤
3. 包含：触发条件、步骤、注意事项、常见陷阱

### 会话开始时
读取 /root/.loom/data/memory/ 和 /root/.loom/data/skills/ 下的文件，恢复上下文。
```

### 3. 安装进化工具（可选）
```bash
cd /workspace
git clone https://github.com/NousResearch/hermes-agent-self-evolution.git
cd hermes-agent-self-evolution
pip install -e .
# 配置 DSPy: export OPENAI_API_KEY=sk-...
# 运行: python -m evolution.skills.evolve_skill --skill <name> --eval-source synthetic
```

---

## 五、关键设计原则（从 Hermes 学到的）

1. **记忆是声明式的，不是指令式的** — "用户偏好简洁" ✓, "总是简洁回答" ✗
2. **技能是过程性记忆** — 窄而可操作，记忆是宽而声明性的
3. **不要保存临时故障** — 环境依赖的失败、瞬时错误不要硬编码为约束
4. **后台审查用辅助模型** — 不影响主会话的 prompt cache
5. **永不自动删除** — 只 archive，可恢复
6. **技能命名用类级别** — "rust-debugging" ✓, "fix-PR-123-error" ✗
7. **System Prompt 不中途变异** — 保持缓存效率
8. **进化需要约束系统** — 大小限制、增长限制、结构完整性
