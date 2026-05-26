# Loom vs Hermes 进化系统实现差异详析

> 审查日期：2025-08-19（源码验证：2026-05-26）
> 审查范围：`loom/src/background_review/` + `cli/src/run/` + `cli/src/review_*.rs` vs `thirdparty/Hermes-Agent/agent/` 全模块
>
> **注意**：`cli/src/run/` 下的 review/curator/evolution 文件均为 2 行 re-export（`pub use loom::background_review::*`），
> 实际实现全部在 `loom/src/background_review/` 中。

---

## 〇、Hermes Agent 模块总览

### 核心架构（`agent/` 目录，~80 个模块）

**入口与主循环**

| 文件 | 功能 |
|------|------|
| `run_agent.py` | AIAgent 主类：模型调用、对话管理、fork 机制 |
| `agent/conversation_loop.py` | `run_conversation()` — 3900 行主循环：model call → tool dispatch → retry → compression → post-turn hooks → background review nudge |
| `agent/agent_init.py` | Agent 初始化、配置加载 |
| `agent/agent_runtime_helpers.py` | 运行时辅助函数 |

**Prompt 与上下文管理**

| 文件 | 功能 |
|------|------|
| `agent/system_prompt.py` | System prompt 组装（identity + platform hints + skills index + context files），每 session 构建一次以保持 prefix cache |
| `agent/prompt_builder.py` | 无状态 prompt 片段拼接 |
| `agent/prompt_caching.py` | Anthropic prompt caching 策略（`system_and_3` 布局，~75% input token 节省） |
| `agent/context_engine.py` | 可插拔上下文引擎基类（默认 compressor，支持 LCM 等第三方引擎） |
| `agent/context_compressor.py` | 自动上下文窗口压缩：用 auxiliary model 做中间轮次摘要 |
| `agent/conversation_compression.py` | 压缩可行性检查、replay compression、session rebuild |
| `agent/context_references.py` | 上下文引用管理 |

**模型与 Provider 适配**

| 文件 | 功能 |
|------|------|
| `agent/anthropic_adapter.py` | Anthropic API 适配 |
| `agent/bedrock_adapter.py` | AWS Bedrock 适配 |
| `agent/codex_responses_adapter.py` | OpenAI Codex Responses API 适配 |
| `agent/codex_runtime.py` | Codex 运行时管理 |
| `agent/gemini_native_adapter.py` | Google Gemini 原生 API 适配 |
| `agent/gemini_cloudcode_adapter.py` | Gemini Cloud Code 适配 |
| `agent/gemini_schema.py` | Gemini schema 转换 |
| `agent/google_code_assist.py` | Google Code Assist 集成 |
| `agent/google_oauth.py` | Google OAuth 认证 |
| `agent/azure_identity_adapter.py` | Azure Identity 认证适配 |
| `agent/moonshot_schema.py` | Moonshot API schema |
| `agent/lmstudio_reasoning.py` | LM Studio reasoning 处理 |
| `agent/model_metadata.py` | 模型元数据（context window、pricing） |
| `agent/chat_completion_helpers.py` | OpenAI ChatCompletion 辅助函数 |

**凭据与安全**

| 文件 | 功能 |
|------|------|
| `agent/credential_pool.py` | 多凭据池：同 provider failover，自动轮换 |
| `agent/credential_sources.py` | 凭据来源解析 |
| `agent/file_safety.py` | 文件操作安全检查 |
| `agent/tool_guardrails.py` | 工具调用循环护栏（无副作用决策引擎） |

**工具执行**

| 文件 | 功能 |
|------|------|
| `agent/tool_executor.py` | 工具调用执行（顺序 + 并发 dispatch） |
| `agent/tool_dispatch_helpers.py` | 工具分发辅助 |
| `agent/tool_result_classification.py` | 工具结果分类 |
| `model_tools.py` | 工具定义注册（toolset 系统：memory、skills、terminal、browser 等） |

**内存与记忆**

| 文件 | 功能 |
|------|------|
| `agent/memory_manager.py` | MemoryManager：编排所有 memory provider，限制一个外部 provider |
| `agent/memory_provider.py` | 可插拔 memory provider 基类（Honcho、Mem0、Supermemory 等） |

**技能系统**

| 文件 | 功能 |
|------|------|
| `agent/skill_utils.py` | 技能元数据工具（轻量，无重依赖） |
| `agent/skill_bundles.py` | 技能包：多技能批量加载别名 |
| `agent/skill_commands.py` | `/skill-name` 斜杠命令处理（CLI + gateway 共享） |
| `agent/skill_preprocessing.py` | SKILL.md 预处理（模板变量替换、inline shell 展开） |

**错误处理与重试**

| 文件 | 功能 |
|------|------|
| `agent/error_classifier.py` | API 错误分类：retry / rotate credential / fallback provider / compress / abort |
| `agent/retry_utils.py` | 重试工具函数 |
| `agent/rate_limit_tracker.py` | 速率限制追踪 |
| `agent/nous_rate_guard.py` | Nous API 速率保护 |

**显示与 UI**

| 文件 | 功能 |
|------|------|
| `agent/display.py` | 终端显示管理 |
| `agent/markdown_tables.py` | Markdown 表格渲染 |
| `agent/i18n.py` | 国际化 |
| `agent/think_scrubber.py` | 思考标签清理 |
| `agent/message_sanitization.py` | 消息清洗 |
| `agent/title_generator.py` | 会话标题生成 |
| `agent/redact.py` | 敏感信息脱敏 |

**其他核心模块**

| 文件 | 功能 |
|------|------|
| `agent/iteration_budget.py` | 迭代预算（线程安全计数器，默认 90 轮） |
| `agent/async_utils.py` | 异步工具函数 |
| `agent/auxiliary_client.py` | Auxiliary model 独立客户端（review/compression/curator 共用） |
| `agent/process_bootstrap.py` | 进程引导 |
| `agent/shell_hooks.py` | Shell 钩子 |
| `agent/trajectory.py` | 轨迹保存与格式转换 |
| `agent/portal_tags.py` | Portal 标签 |
| `agent/stream_diag.py` | 流式诊断 |
| `agent/subdirectory_hints.py` | 子目录提示 |
| `agent/onboarding.py` | 新用户引导 |
| `agent/insights.py` | 会话洞察引擎（token 消耗、成本估算、工具使用模式） |
| `agent/usage_pricing.py` | 用量计价 |
| `agent/account_usage.py` | 账户用量追踪 |
| `agent/manual_compression_feedback.py` | 手动压缩反馈 |
| `agent/image_gen_provider.py` / `image_gen_registry.py` / `image_routing.py` | 图片生成 |
| `agent/video_gen_provider.py` / `video_gen_registry.py` | 视频生成 |
| `agent/web_search_provider.py` / `web_search_registry.py` | Web 搜索 |
| `agent/browser_provider.py` / `browser_registry.py` | 浏览器自动化 |

**LSP 子系统**（`agent/lsp/`）

| 文件 | 功能 |
|------|------|
| `cli.py` | LSP CLI 入口 |
| `client.py` | LSP 客户端 |
| `server.py` | LSP 服务端 |
| `manager.py` | LSP 服务器管理 |
| `protocol.py` | LSP 协议实现 |
| `workspace.py` | 工作区管理 |
| `range_shift.py` | 文本范围偏移 |
| `eventlog.py` | 事件日志 |
| `install.py` | LSP 安装 |
| `reporter.py` | LSP 报告 |

**传输层**（`agent/transports/`）

| 文件 | 功能 |
|------|------|
| `base.py` | 传输基类 |
| `chat_completions.py` | OpenAI Chat Completions 传输 |
| `anthropic.py` | Anthropic 传输 |
| `bedrock.py` | Bedrock 传输 |
| `codex.py` / `codex_app_server.py` / `codex_app_server_session.py` / `codex_event_projector.py` | Codex 传输 |
| `hermes_tools_mcp_server.py` | Hermes MCP 工具服务 |
| `types.py` | 传输层类型 |

**插件系统**（`plugins/`）

| 目录 | 功能 |
|------|------|
| `browser/` | 浏览器插件（browser_use、browserbase、firecrawl） |
| `context_engine/` | 第三方上下文引擎 |
| `disk-cleanup/` | 磁盘清理 |
| `memory/` | Memory provider 插件（Honcho、Hindsight、Mem0、Supermemory 等） |

---

### 自我进化子系统（4 个核心模块）

以下模块构成 Hermes Agent 的完整自我进化链：

```
会话结束
  → background_review.py (582行，每 N 轮触发，forked agent 做 memory+skill review)
  → curator.py (1781行，空闲时触发，LLM-driven umbrella-building + 生命周期管理)
  → curator_backup.py (693行，curator 运行前快照 + 回滚)
  → darwinian-evolver (optional skill，手动触发的 GEPA 进化)
```

**1. `agent/background_review.py`** (582 行) — 后台 Memory/Skill Review

- **触发**：每 10 个 user turn（`_memory_nudge_interval`）/ 每 10 次工具迭代（`_skill_nudge_interval`）
- **机制**：fork AIAgent（继承 provider、credentials、cached system prompt），运行在 daemon thread
- **工具白名单**：`memory` + `skills` toolset（`set_thread_tool_whitelist`）
- **三套 Prompt**：`MEMORY_REVIEW_PROMPT` / `SKILL_REVIEW_PROMPT` / `COMBINED_REVIEW_PROMPT`
- **反模式保护**：不保存环境依赖故障、负面工具断言、瞬时错误、一次性任务叙事
- **输出**：`💾 Self-improvement review: <summary>`
- **核心函数**：
  - `spawn_background_review()` — 入口，daemon thread 启动
  - `_do_background_review()` — fork agent + 运行 review 循环
  - `summarize_background_review_actions()` — 动作总结 + 去重

**2. `agent/curator.py`** (1781 行) — 技能生命周期管理 + LLM Consolidation

- **触发**：`maybe_run_curator()` — `interval_hours`（默认 7 天）+ `min_idle_hours`（默认 2 小时）
- **两阶段**：
  1. **Auto-transitions**（纯规则，无 LLM）：Active → Stale（30天） → Archived（90天），支持反向 Reactivation
  2. **LLM Consolidation Pass**：forked AIAgent 运行 `CURATOR_REVIEW_PROMPT`（114行），执行 umbrella-building
- **分类审计系统**：三层仲裁（model-declared `absorbed_into` > YAML structured block > tool-call heuristic）
- **报告系统**：`logs/curator/{timestamp}/run.json` + `REPORT.md` + `cron_rewrites.json`
- **Cron 引用重写**：consolidation 后自动更新 cron job 中的技能引用
- **状态管理**：`.curator_state`（`last_run_at`, `run_count`, `paused`, `last_report_path`）
- **安全**：Pinned skills 跳过所有自动转换，`curator_backup` 快照
- **核心函数**：
  - `maybe_run_curator()` — 空闲触发入口
  - `run_curator_review()` — 单次 curator pass
  - `_run_llm_review()` — fork AIAgent 做 LLM review
  - `_classify_removed_skills()` — tool-call heuristic 分类
  - `_reconcile_classification()` — 三层分类仲裁
  - `_write_run_report()` — 报告写入

**3. `agent/curator_backup.py`** (693 行) — Curator 快照与回滚

- **快照**：curator 运行前 tar.gz 打包 `~/.hermes/skills/` 到 `.curator_backups/<utc-iso>/`，同时备份 `cron/jobs.json` 为 `cron-jobs.json`
- **包含**：所有 SKILL.md + 子目录、`.usage.json`、`.archive/`、`.curator_state`、`.bundled_manifest`
- **排除**：`.curator_backups/`（避免递归）、`.hub/`（由 hub 管理）
- **回滚**：选择快照恢复，当前 skills tree 移到新快照（回滚本身可撤销），同时恢复 cron job 的技能引用
- **核心函数**：`snapshot_skills()` / `rollback_to_snapshot()`
- **配置**：`DEFAULT_KEEP = 5`（最多保留 5 个快照）

**4. `optional-skills/research/darwinian-evolver/`** — GEPA 进化（Optional Skill）

- **性质**：可选技能，用户手动安装
- **实现**：Python wrapper，通过 `subprocess`/`uv run` 调用 `imbue-ai/darwinian_evolver`（AGPL-3.0 外部工具）
- **License 隔离**：严格隔离，不 import 到 Hermes core
- **用途**：Prompt / regex / SQL / code evolution via evolutionary search

---

### 进化相关辅助模块

| 文件 | 在进化中的角色 |
|------|---------------|
| `agent/auxiliary_client.py` | review / compression / curator 共用的 auxiliary model 客户端 |
| `agent/prompt_caching.py` | background review fork 继承 cached system prompt 以节省 ~26% 成本 |
| `agent/credential_pool.py` | curator/review fork 的凭据继承来源 |
| `agent/skill_utils.py` | `agent_created_report()` 提供技能元数据给 curator |
| `agent/memory_manager.py` | background review 写入 memory 的入口 |

---

### Loom 模块对照表

以下按 Hermes 模块分组，逐一列出 Loom 的对应实现（或标记缺失）。

| Hermes 模块 | 功能 | Loom 对应 | 状态 |
|-------------|------|-----------|------|
| **自我进化：Review** | | | |
| `background_review.py` (582行) | 后台 review | `loom/src/background_review/agent_loop.rs` (207行) + `prompts.rs` (217行) + `tools.rs` (428行) + `workflow.rs` (359行) | ✅ 有（`cli/src/run/` 下为 re-export） |
| **自我进化：Curator** | | | |
| `curator.py` (1781行) | 生命周期 + LLM consolidation | `loom/src/background_review/curator.rs` (335行) | ⚠️ 大幅简化（无 LLM pass、无分类审计、无报告） |
| `curator_backup.py` | 快照回滚 | — | ❌ 缺失 |
| **自我进化：Evolution** | | | |
| `darwinian-evolver/` (optional) | GEPA 进化 | `loom/src/background_review/evolution.rs` (32行，仅 config 类型) | ✅ 有（内置 vs optional） |
| **Loom 独有** | | | |
| — | Agent 图框架 | `loom/src/graph/` + `pregel/` | ✨ Hermes 无 |
| — | 多 Agent 模式 (ToT/GoT/DUP) | `loom/src/agent/tot/` + `got/` + `dup/` | ✨ Hermes 无 |
| — | Goal Runner | `loom/src/goal_runner/` | ✨ Hermes 无 |
| — | MCP 工具源 | `loom/src/tool_source/mcp/` | ✨ Hermes 无 |
| — | Skill security 校验 | `loom/src/background_review/security.rs` | ✨ Hermes 无 |
| — | Review history 持久化 | `loom/src/background_review/history.rs` (`cli/src/review_history.rs` re-export) | ✨ Hermes 无 |
| — | Observability | `loom/src/background_review/observability.rs` | ✨ Hermes 无 |
| — | CLI review 子命令 | `cli/src/review_cmd.rs` (419行) + `cli/src/review_skill_cmd.rs` (147行) | ✨ Hermes 无 |
| — | Telegram 工具 | `loom/src/tools/telegram/` | ✨ Hermes 无 |
| — | Task 管理 | `loom/src/tools/task/` | ✨ Hermes 无 |
| — | Todo 管理 | `loom/src/tools/todo/` | ✨ Hermes 无 |

**图例**：✅ 完整实现 | ⚠️ 部分实现 | ❌ 缺失 | ✨ Loom 独有

---

## 一、Review Prompt 逐字差异

### 1.1 MEMORY_REVIEW_PROMPT

**结论：完全一致，无差异。**

### 1.2 SKILL_REVIEW_PROMPT

Hermes 40 行 / 5637 字符，Loom ~20 个语义段。以下为全部差异点：

| 编号 | 差异描述 | Hermes 原文 | Loom 处理 | 影响 |
|------|----------|-------------|-----------|------|
| S1 | 元指导句缺失 | `"This shapes HOW you update, not WHETHER you update."` | 删除 | Agent 可能过度犹豫是否该更新 |
| S2 | 挫败信号示例缩减 | 列出 7 个示例：`'stop doing X'`, `'this is too verbose'`, `'don't format like this'`, `'why are you explaining'`, `'just give me the answer'`, `'you always do Y and I hate it'`, `'remember this'` | 仅保留 3 个：`'stop doing X'`, `'don't format like this'`, `'I hate when you Y'` | 漏检部分挫败信号 |
| S3 | **workflow correction signal 整条缺失** | `"User corrected your workflow, approach, or sequence of steps. Encode the correction as a pitfall or explicit step in the skill that governs that class of task."` | 完全删除 | 工作流纠正不会被编码为技能更新 |
| S4 | tool-usage pattern 信号缺失 | `"...tool-usage pattern emerged that a future session would benefit from. Capture it."` | 只保留 "debugging path emerged" | 工具使用模式不会被捕获 |
| S5 | skill 过时信号简化 | `"...got loaded or consulted this session turned out to be wrong, missing a step, or outdated. Patch it NOW."` | `"wrong, missing, or outdated — patch it now"` | 缺少 "this session" 和 "a step" 上下文 |
| S6 | Preference 1 描述缩短 | 详细说明了查找 loaded skills 的方法和原因 | 精简为一句 | 指导不够具体 |
| S7 | Preference 2 缺少操作指导 | `"Add a subsection, a pitfall, or broaden a trigger."` | 只保留 `"Patch it."` | Agent 不知道如何 patch |
| S8 | Preference 3 support file 描述缩短 | 每种类型 2-3 行说明 + `skill_manage action=write_file` 用法 | 每种类型半行描述，删除了具体 API 调用说明 | Agent 可能不知道如何正确创建 support file |
| S9 | Preference 4 命名规则简化 | `"The name MUST be at the class level. The name MUST NOT be..."` 独立强调句 | 合并到一行 | 命名约束不够强 |
| S10 | **User-preference embedding 段整段缺失** | 完整段说明 style/format/workflow preference 应写入 SKILL.md body 而非仅 memory | 完全删除 | 用户偏好可能只写到 memory 而不更新 skill |
| S11 | Overlap 检测描述差异 | `"note it in your reply"` | `"mention it"` | 轻微 |
| S12 | **Protected skills 整段缺失** | 6 行完整规则：Bundled/Hub-installed/Pinned skills 不可编辑 | 完全删除 | **review agent 可能修改受保护技能** |
| S13 | Do NOT capture 解释性文字删除 | `"The user can fix these — they are not durable rules"` + 更详细的负面声明示例 | 删除了解释文字 | Agent 可能不理解为什么不能捕获 |
| S14 | 结尾语差异 | `"Nothing to save.' is a real option but should NOT be the default. If the session ran smoothly with no corrections and produced no new technique, just say 'Nothing to save.' and stop. Otherwise, act."` | 简化为一行 | 行为倾向可能不同 |

### 1.3 COMBINED_REVIEW_PROMPT

比 SKILL 更接近原版，但仍有删减：

| 编号 | 差异描述 | Hermes | Loom | 影响 |
|------|----------|--------|------|------|
| C1 | Preference 3 support file 描述缩短 | references 展开了 `"quoted research, API docs excerpts, domain notes"` 和 `"written concise and task-focused"`；templates/scripts 各有 1-2 行描述 | references 只保留 `"condensed knowledge banks"`，templates/scripts 删除了详细描述 | Agent 创建的 support file 质量可能降低 |
| C2 | Preference 4 缺少 fallback 指导 | `"If the name only fits today's task, fall back to (1), (2), or (3)."` | 删除 | Agent 可能创建不合适的 skill name |
| C3 | **Protected skills 整段缺失** | 同 SKILL 的 S12 | 完全删除 | 同 S12 |
| C4 | Do NOT capture 缺少解释 | `"The user can fix these — they are not durable rules."` | 删除 | 同 S13 |
| C5 | 负面声明示例缩减 | 包含 `'cannot use Y from execute_code'` | 删除该示例 | 轻微 |

---

## 二、System Prompt

| 方面 | Hermes | Loom |
|------|--------|------|
| 来源 | 继承父 agent 的 `_cached_system_prompt`（完整角色定义、工具列表、memory/skills 内容等） | 独立的 `build_system_prompt()` 返回精简版英文指令（~15 行） |
| Prefix cache | 命中父 agent 的 prefix cache（实测 ~26% 成本节省） | 每次重新构建，无法命中 prefix cache |
| 内容丰富度 | 包含完整的 agent 身份、工具描述、加载的 skills 内容 | 仅列出 review 工具名称和基本规则 |

---

## 三、工具白名单与执行

| 方面 | Hermes | Loom |
|------|--------|------|
| 实现方式 | `set_thread_tool_whitelist()` — 线程级运行时拦截，基于 `enabled_toolsets=["memory", "skills"]` 动态获取工具名 | `ReviewToolExecutor::execute()` — 硬编码 match 分发 10 个工具名 |
| 拒绝行为 | 返回 deny 消息：`"Background review denied non-whitelisted tool: {tool_name}."` | 返回 `{"success": false, "error": "Unknown tool: ..."}` |
| 灵活性 | 新增工具自动包含在 toolset 中 | 需手动添加到 `ReviewToolExecutor` + `review_tool_specs()` |
| 工具定义来源 | `get_tool_definitions(enabled_toolsets=...)` 动态获取 | `review_tool_specs()` 静态定义 |
| 工具数量 | 动态（取决于注册的 memory + skills 工具数） | 固定 10 个 |

---

## 四、Review Agent 循环

| 方面 | Hermes | Loom |
|------|--------|------|
| 循环实现 | 复用 `AIAgent.run_conversation()` 完整循环 | `AgentReviewRunner::run_with_refs()` 手动实现循环 |
| 消息格式 | OpenAI 兼容 dict | `loom::message::Message` enum |
| 工具调用解析 | Agent 内置处理 | 手动 `serde_json::from_str` → `executor.execute()` |
| 消息记录 | `review_agent._session_messages` 自动收集 | `session_messages: Vec<serde_json::Value>` 手动构建 |
| 状态抑制 | `suppress_status_output = True` — 防止中间状态泄露 | 无此机制 |
| 危险命令审批 | `_bg_review_auto_deny` 回调自动拒绝 | 无此机制（沙箱化设计，不触发 shell） |
| Memory provider 隔离 | `skip_memory=True` 避免触发外部 memory 插件 (honcho/mem0/supermemory) | 无外部 memory provider 集成 |
| Session pinning | `review_agent.session_start/session_id = agent.session_start/session_id` 确保一致性 | 无此概念 |
| API mode 降级 | `codex_app_server` → `codex_responses` 降级 | 无 codex 集成 |
| Credential pool | 继承 `agent._credential_pool` | 无此概念 |
| Memory write origin | `_memory_write_origin = "background_review"` | 无 origin 标记 |
| Memory write context | `_memory_write_context = "background_review"` | 无 context 标记 |

---

## 五、触发机制

| 方面 | Hermes | Loom |
|------|--------|------|
| Memory 触发 | 基于 turn 计数器：`_memory_nudge_interval`（默认 10），每 N 个 user turn 触发一次 | 每次 turn 都触发（`review_memory: true` 默认开启） |
| Skill 触发 | 基于迭代计数器：`_skill_nudge_interval`（默认 10），累积 N 次工具迭代后触发 | 每次 turn 都触发（`review_skills: true` 默认开启） |
| 前置条件 | `final_response and not interrupted` + 工具名在 `valid_tool_names` 中 | `stop_reason == EndTurn && !reply.is_empty()` |
| 最短会话长度 | 无显式检查 | `min_session_chars: 200` |
| 节流方式 | Nudge interval（10 轮/次） | `min_session_chars`（200 字符） |

**成本影响**：Hermes 平均每 10 轮做一次 review，Loom 每轮必做（仅排除 <200 字符的短会话），review API 调用量约为 Hermes 的 10 倍。

---

## 六、会话内容传入

| 方面 | Hermes | Loom |
|------|--------|------|
| 内容来源 | `messages_snapshot=list(messages)` — **完整消息历史**（所有 user/assistant/tool/system 消息） | `format!("User: {}\n\nAssistant: {}", user_msg, reply)` — **仅最后一轮** |
| 内容丰富度 | 包含所有历史轮次的对话、工具调用、工具结果 | 只有当前 user message 和 assistant reply 的文本 |

**这是影响 review 质量的最大差异。** Loom 的 review agent 看不到历史上下文，无法：
- 识别跨多轮的用户偏好变化
- 检测之前加载过的 skills（Preference 1 依赖此信息）
- 追踪调试路径的完整上下文
- 理解复杂任务的完整演进过程

---

## 七、Summary / Action 总结

| 方面 | Hermes `summarize_background_review_actions` | Loom `AgentReviewRunner::summarize_actions` |
|------|-----------------------------------------------|----------------------------------------------|
| 输入 | `review_messages` (tool 消息列表) + `prior_snapshot` (去重用) | `actions: &[ReviewAction]` (预收集列表) |
| 去重 | 基于 `tool_call_id` + `content` 相等性，过滤 `prior_snapshot` 中的已有消息 | 无去重机制 |
| 成功判断 | JSON 解析 `data["success"]` + message 内容关键词匹配 | `summary.contains()` 关键词匹配 |
| 关键词 | `created` / `updated` / `added` / `removed` / `replaced` / `Entry added` | `created` / `updated` / `appended` / `patched` / `replaced` / `added` / `removed` |
| 分隔符 | ` · ` (中点) | `"; "` (分号+空格) |
| 展示前缀 | `💾 Self-improvement review:` | `📚 Background review:` |
| 回调机制 | `background_review_callback` 通知 gateway/websocket | 无回调，仅 stderr 打印 |
| Hermes `_safe_print` | 支持 TUI 线程安全打印 | 直接 `eprintln!` |

---

## 八、凭据与模型解析

| 方面 | Hermes | Loom |
|------|--------|------|
| 凭据来源 | 从 `agent._current_main_runtime()` 继承 provider/base_url/api_key/api_mode | 从 `opts` 或环境变量 `OPENAI_BASE_URL`/`OPENAI_API_KEY` 解析 |
| Strong tier | 无此概念 | `resolve_review_model` 尝试从 session model 解析 Strong tier |
| Session model 解析 | — | `resolve_session_model` 硬编码 `ModelTier::Standard`，不使用实际会话 tier |
| 默认模型 | 继承父 agent 的 model | `gpt-4o-mini` |
| Provider 差异处理 | codex 降级逻辑 | 无 |

---

## 九、后台执行模型

| 方面 | Hermes | Loom |
|------|--------|------|
| 执行方式 | `threading.Thread(daemon=True)` — 操作系统线程 | `tokio::spawn` — 异步任务 |
| 输出抑制 | `contextlib.redirect_stdout/stderr` + `suppress_status_output` | 依赖 tracing level 控制 |
| 进程退出行为 | daemon thread 随进程自动终止（可能丢失进行中的 review） | `PendingReviewRegistry` + `wait_for_pending_reviews()` 等待完成 |
| Review 后续动作 | 无（review 是最终步骤） | 自动触发 `run_curator_if_needed` + `run_evolution_if_eligible` |
| 持久化 | 无 review 历史记录 | `ReviewHistory` JSONL + `ObservabilityStore` 可观测性 |
| Memory provider 清理 | `review_agent.shutdown_memory_provider()` + `review_agent.close()` | 无需清理（无外部 provider） |

---

## 十、Curator 系统对比

Hermes `agent/curator.py` 有 1781 行，Loom `curator.rs` 有 336 行。两者都有 curator 但实现深度差距极大。

### 10.1 架构对比

| 方面 | Hermes（1781行） | Loom（336行） |
|------|------------------|---------------|
| 触发方式 | 空闲触发：`maybe_run_curator()` 检查 `interval_hours`（默认7天）+ `min_idle_hours`（默认2小时），agent 空闲时才运行 | Review 后自动触发 `run_curator_if_needed`，无空闲检测 |
| LLM pass | 完整的 forked AIAgent 做 consolidation review（`CURATOR_REVIEW_PROMPT` 114行 prompt） | 无 LLM pass，纯规则逻辑 |
| 状态字段 | `last_run_at`, `last_run_duration_seconds`, `last_run_summary`, `last_run_summary_shown_at`, `last_report_path`, `paused`, `run_count` | `skill_last_used[name]` HashMap |
| 配置来源 | `~/.hermes/config.yaml` → `curator.*` 配置段 | `CuratorConfig` 硬编码默认值 |
| 暂停/恢复 | `set_paused(bool)` / `is_paused()` | 无此机制 |
| Pinned 保护 | Pinned skills 跳过所有自动转换 | 无 pinned 概念 |
| Source 过滤 | 仅处理 agent-created skills（`is_agent_created` 过滤） | 按 `Source::Auto` vs manual 区分 stale_days |
| Reactivation | Stale skill 被重新使用后自动恢复为 Active | 无 reactivation 逻辑 |

### 10.2 生命周期管理

| 方面 | Hermes | Loom |
|------|--------|------|
| Stale 默认天数 | 30 天 | Auto: 60天, Manual: 30天 |
| Archive 默认天数 | 90 天 | 90 天 |
| 依据字段 | `last_activity_at` from `skill_usage.py`（fallback `created_at`） | `skill_last_used[name]`（首次无记录则 `u32::MAX` → 立即 stale） |
| 转换方向 | Active → Stale → Archived，**支持反向** Stale → Active | Active → Stale → Archived，**无反向** |
| Never-used 处理 | 以 `created_at` 为锚点，新技能不会立即 stale | 无 `last_used` 记录时 `days_since = u32::MAX`，**新技能会立即被标记为 stale** |

**Loom 的 "never-used = u32::MAX" 是一个 bug**：新创建的技能如果没有被 `touch_skill` 过，会在首次 curator run 时立即被标记为 stale（因为 `days_since = u32::MAX > stale_days`）。Hermes 以 `created_at` 为锚点避免了这个问题。

### 10.3 LLM Consolidation Pass

| 方面 | Hermes | Loom |
|------|--------|------|
| 是否有 LLM pass | **有** — `CURATOR_REVIEW_PROMPT`（~115行）指导 forked agent 做技能合并 | **无** — 纯规则逻辑 |
| Prompt 内容 | 详细的 umbrella-building 指令：识别 prefix cluster → merge/create umbrella → demote to support file → archive siblings | 不适用 |
| 合并策略 | 三种：a) merge into existing umbrella b) create new umbrella c) demote to references/templates/scripts | 无合并逻辑 |
| Overlap 检测 | LLM 自主判断（基于语义理解） | Jaccard word-level similarity on description + triggers |
| 工具集 | `skills_list`, `skill_view`, `skill_manage(patch/create/delete/write_file)`, `terminal` | 无 |
| 输出格式 | Human-readable summary + structured YAML block（`consolidations` + `prunings`） | `CuratorReport`（active/stale/archived/overlapping 列表） |

### 10.4 分类与审计（Hermes 独有）

Hermes curator 有复杂的 post-hoc 分类系统，Loom 完全没有：

- **`_classify_removed_skills`**：扫描 tool call，判断被 archive 的 skill 是 "consolidated"（内容已吸收到 umbrella）还是 "pruned"（纯归档）
- **`_parse_structured_summary`**：解析 LLM 输出的 YAML block，提取 `consolidations` 和 `prunings`
- **`_extract_absorbed_into_declarations`**：从 `skill_manage(action=delete)` 的 `absorbed_into` 参数提取权威分类信号
- **`_reconcile_classification`**：三层分类仲裁：model-declared `absorbed_into` > YAML structured block > tool-call heuristic
- **`_build_rename_summary`**：生成用户可见的 "where did my skills go?" 摘要
- **`_write_run_report`**：写入 `logs/curator/{timestamp}/run.json` + `REPORT.md`
- **`CURATOR_DRY_RUN_BANNER`**：dry-run 模式下的禁止性指令横幅

### 10.5 Dry-run 模式

| 方面 | Hermes | Loom |
|------|--------|------|
| 自动转换 | 跳过 `apply_automatic_transitions`，只计数 | 跳过 lifecycle 更新 |
| LLM pass | 添加 `CURATOR_DRY_RUN_BANNER`，禁止 mutating action | 不适用（无 LLM pass） |
| 状态更新 | **不**更新 `last_run_at` 和 `run_count` | **不**保存 state |
| 报告 | 仍写入 `REPORT.md`（描述 WOULD take 的动作） | 不写入报告 |
| Pre-run snapshot | 非 dry-run 时调用 `curator_backup.snapshot_skills` | 无快照 |

### 10.6 报告系统（Hermes 独有）

Hermes 写入完整报告到 `~/.hermes/logs/curator/{YYYYMMDD-HHMMSS}/`：
- `run.json`：机器可读完整记录（started_at, duration, model, tool_calls, consolidated/pruned 列表, cron rewrites）
- `REPORT.md`：人类可读 markdown 报告（auto-transitions, consolidation summary, pruned list, recovery instructions）
- `cron_rewrites.json`：cron job 技能引用重写记录

Loom 无任何报告文件输出。

### 10.7 Cron Job 引用重写（Hermes 独有）

Hermes curator 在 consolidation 后自动重写 cron job 中的技能引用：
- 如果 skill X 被合并到 umbrella Y，所有引用 X 的 cron job 自动更新为引用 Y
- 如果 skill X 被 pruned，引用 X 的 cron job 中该引用被移除
- 记录到 `cron_rewrites.json`

Loom 无此机制。

### 10.8 模型/凭据解析

| 方面 | Hermes | Loom |
|------|--------|------|
| 模型配置 | `auxiliary.curator.{provider,model,api_key,base_url}` → legacy `curator.auxiliary.*` → main model | 不适用（无 LLM pass） |
| 凭据隔离 | `resolve_runtime_provider` 支持独立 API key/base_url | 不适用 |
| 迭代上限 | `max_iterations=9999`（curator 需要大量 API 调用处理数百个技能） | 不适用 |

---

## 十一、Evolution Trigger 对比

| 方面 | Hermes | Loom |
|------|--------|------|
| GEPA/darwinian-evolver | **optional skill**（`optional-skills/research/darwinian-evolver/`），用户手动调用 | **内置模块** `evolution_trigger.rs`，review 后自动触发 |
| 触发方式 | 用户通过 skill 的 SKILL.md 指导，手动安装和运行 | `run_evolution_if_eligible` 检查 `train.jsonl` 是否满足 `min_examples` |
| 实现语言 | Python wrapper around [imbue-ai/darwinian_evolver](https://github.com/imbue-ai/darwinian_evolver)（AGPL-3.0 外部工具） | Rust 内置 GEPA 优化器 |
| 问题类型 | Prompt/regex/SQL/code evolution via evolutionary search | Skill body optimization |
| License 隔离 | 严格隔离：`subprocess`/`uv run` 调用，不 import 到 Hermes core | 内置，无隔离需求 |

---

## 十二、Loom 独有功能（Hermes 中不存在）

| 模块 | 文件 | 功能说明 |
|------|------|----------|
| Json Review 模式 | `review.rs` | 中文 prompt + JSON 结构化输出解析的 review 模式（残留，未被实际使用） |
| Review History | `review_history.rs` | JSONL 格式的 review 历史记录持久化 |
| Observability | `observability.rs` | Review/evolution 可观测性指标收集 |
| CLI Review 命令 | `review_cmd.rs` / `review_skill_cmd.rs` | `loom review` 子命令手动触发 review |
| Security 校验 | `security.rs` | `validate_skill_create` / `validate_skill_path` 安全校验 |

---

## 十三、影响评估与优先级建议

### P0 — 影响正确性

1. **会话内容传入不足**（第六项）：`cli/src/run/agent.rs:349` 只传最后一轮文本。应传入完整消息历史或至少最近 N 轮。
2. **Protected skills 规则缺失**（S12/C3）：`ReviewToolExecutor` 的 `skill_edit`/`skill_patch`/`skill_create` 缺少对 bundled/pinned 技能的保护检查。
3. **Workflow correction signal 缺失**（S3）：Agent 不会将工作流纠正编码为技能更新。
4. **Curator never-used bug**（10.2）：新技能 `days_since = u32::MAX` 导致立即被标记 stale。应以 `created_at` 为 fallback 锚点。
5. **Curator 无 Reactivation**（10.2）：Stale 技能被重新使用后无法恢复 Active。

### P1 — 影响效率/成本

6. **每轮都触发 review**（第五项）：应实现类似 Hermes 的 nudge interval 机制，默认每 10 轮触发一次。
7. **无 prefix cache 优化**（第二项）：system prompt 每次重新构建，无法命中缓存。
8. **resolve_session_model 硬编码 Standard tier**（第八项）：应使用实际会话的 tier。
9. **Curator 缺少空闲触发**（10.1）：应实现 Hermes 的 interval + idle gating，而非每次 review 后自动触发。

### P2 — 影响质量

10. **User-preference embedding 段缺失**（S10）：偏好可能只写到 memory 不更新 skill。
11. **Preference 2/3/4 操作指导不充分**（S7/S8/S9/C1/C2）：Agent 可能不知道如何正确执行更新。
12. **Curator 相似度检测基于 word-level Jaccard**：对语义相似度检测能力有限。
13. **Review 后续动作链（Curator + Evolution）在 Hermes 中不存在**：需评估是否应作为 review 的副作用自动触发。

### P3 — 功能缺失（需要较大工作量）

14. **Curator LLM Consolidation Pass**（10.3）：Loom 缺少 Hermes 的 LLM-driven umbrella-building 能力。
15. **Curator 分类审计系统**（10.4）：缺少 post-hoc classification、absorbed_into declaration、reconciliation。
16. **Curator 报告系统**（10.6）：缺少 run.json + REPORT.md 输出。
17. **Curator 暂停/恢复**（10.1）：缺少 paused 机制。

---

## 十四、Loom 改进计划

基于前述差异分析，按四个阶段规划改进。每个阶段有明确的交付物和验收标准。

### Phase 1：正确性修复（1-2 天）

> 修复影响行为正确性的 bug 和缺失，不改架构。

**1.1 Review 会话内容传入不足**

- **现状**：`cli/src/run/agent.rs` 只传 `format!("User: {}\n\nAssistant: {}", user_msg, reply)`，仅最后一轮
- **目标**：传入完整消息历史（或至少最近 N 轮，N 可配置，默认全部）
- **修改文件**：
  - `cli/src/run/background_review.rs` — `spawn_background_review` 改为接收 `Vec<Message>` 或 `Vec<serde_json::Value>`
  - `loom/src/background_review/workflow.rs` — `run_background_review_workflow` 将消息历史注入 user message
  - `loom/src/background_review/agent_loop.rs` — `AgentReviewRunner::run_with_refs` 使用完整历史
- **验收**：Review agent 能看到所有历史 user/assistant 消息，能正确识别跨轮次的偏好变化

**1.2 Curator never-used stale bug**

- **现状**：`skill_last_used` 无记录时 `days_since = u32::MAX`，新技能立即 stale
- **目标**：以技能文件 `created_at`（文件修改时间或 metadata）为 fallback 锚点
- **修改文件**：
  - `loom/src/background_review/curator.rs` — `compute_days_since` 函数增加 `created_at` fallback
- **验收**：新创建的技能在 stale_days 内不会被标记 stale

**1.3 Curator Reactivation**

- **现状**：Stale 技能被重新使用后无法恢复 Active
- **目标**：`touch_skill` 检查当前状态，若为 Stale 则恢复为 Active
- **修改文件**：
  - `loom/src/background_review/curator.rs` — `touch_skill` 增加状态检查和恢复逻辑
- **验收**：Stale 技能被 touch 后恢复 Active，CuratorReport 中体现 reactivation

**1.4 Protected skills 规则**

- **现状**：Review agent 可能修改 bundled/pinned 技能
- **目标**：`ReviewToolExecutor` 的 `skill_edit`/`skill_patch`/`skill_create`/`skill_delete` 检查技能来源，拒绝修改非 agent-created 的技能
- **修改文件**：
  - `loom/src/background_review/tools.rs` — 每个 skill 操作前增加 source 检查
  - `loom/src/background_review/prompts.rs` — 补回 Hermes 的 Protected skills 段落（S12/C3）
- **验收**：Review agent 尝试修改 bundled skill 时返回拒绝消息

### Phase 2：Prompt 与效率优化（2-3 天）

> 补全删减的 prompt 内容，优化触发频率和成本。

**2.1 SKILL_REVIEW_PROMPT 补全**

- **目标**：补回 Hermes 原版的 14 个差异点（S1-S14），优先级：
  - S3 workflow correction signal（影响技能更新覆盖率）
  - S10 user-preference embedding 段（影响偏好持久化）
  - S12 protected skills 段（Phase 1.4 的 prompt 侧配合）
  - S7/S8 preference 2/3 操作指导（影响更新质量）
  - S2 信号示例扩充、S4 tool-usage pattern、S9 命名规则（次要）
- **修改文件**：
  - `loom/src/background_review/prompts.rs` — `SKILL_REVIEW_PROMPT` 常量
- **验收**：SKILL prompt 与 Hermes 差异缩小到仅排版/格式级别

**2.2 COMBINED_REVIEW_PROMPT 补全**

- **目标**：补回 C1-C5 差异点，优先 C2 fallback 指导和 C3 protected skills
- **修改文件**：同 2.1
- **验收**：COMBINED prompt 覆盖 Hermes 的所有操作指导

**2.3 Review 触发频率节流**

- **现状**：每轮必触发 review，成本约为 Hermes 的 10 倍
- **目标**：实现 nudge interval 机制，默认每 10 轮触发一次
- **修改文件**：
  - `loom/src/background_review/workflow.rs` — 增加 `turn_counter` + `nudge_interval` 配置
  - `cli/src/run/background_review.rs` — 传递 turn 计数
- **配置**：
  ```
  nudge_interval: u32 = 10  // 每 N 个 user turn 触发一次
  min_session_chars: u32 = 200  // 保留现有最短长度检查
  ```
- **验收**：Review 触发频率从每轮降至每 10 轮，可配置

**2.4 resolve_session_model 使用实际 tier**

- **现状**：硬编码 `ModelTier::Standard`
- **目标**：从会话的实际 model 配置中解析 tier
- **修改文件**：
  - `loom/src/background_review/workflow.rs` — `resolve_session_model` 接收实际 session model
- **验收**：Review 使用与主会话相同 tier 的模型

### Phase 3：Curator 增强（1-2 周）

> 将 Curator 从纯规则系统升级为支持 LLM-driven consolidation 的完整系统。

**3.1 Curator 状态管理增强**

- **目标**：替换当前的 `skill_last_used` HashMap 为完整的 CuratorState
- **修改文件**：
  - `loom/src/background_review/curator.rs` — 新增 `CuratorState` 结构体
- **状态字段**：
  ```rust
  struct CuratorState {
      last_run_at: Option<DateTime<Utc>>,
      run_count: u32,
      paused: bool,
      skill_last_used: HashMap<String, DateTime<Utc>>,
      skill_created_at: HashMap<String, DateTime<Utc>>,
  }
  ```
- **验收**：Curator 状态持久化到 `curator/state.json`，支持暂停/恢复

**3.2 Curator 空闲触发**

- **目标**：实现 Hermes 的 interval + idle gating，替代当前的 review 后自动触发
- **修改文件**：
  - `loom/src/background_review/curator.rs` — 新增 `maybe_run_curator` 函数
  - `loom/src/background_review/workflow.rs` — 调用改为条件触发
- **配置**：
  ```
  interval_hours: u64 = 168  // 7 天
  min_idle_minutes: u64 = 120  // 2 小时空闲
  ```
- **验收**：Curator 仅在间隔足够且空闲时运行

**3.3 Curator LLM Consolidation Pass**

- **目标**：实现 Hermes 的 umbrella-building 能力
- **分步**：
  1. 编写 `CURATOR_REVIEW_PROMPT` Rust 版（翻译 Hermes 的 114 行 prompt）
  2. 实现 `run_llm_review` — fork review agent 做 consolidation
  3. 实现 overlap 检测 → LLM 判断 → merge/create umbrella → archive siblings 流程
- **修改文件**：
  - `loom/src/background_review/curator.rs` — 新增 `run_llm_consolidation` 函数
  - `loom/src/background_review/prompts.rs` — 新增 `CURATOR_REVIEW_PROMPT` 常量
- **验收**：Curator 能自动合并语义重叠的技能为 umbrella skill

**3.4 Curator 报告系统**

- **目标**：每次 curator run 输出 `run.json` + `REPORT.md`
- **输出路径**：`~/.loom/data/curator/logs/{YYYYMMDD-HHMMSS}/`
- **修改文件**：
  - `loom/src/background_review/curator.rs` — 新增 `write_run_report` 函数
- **验收**：用户可通过 `loom curator report` 查看最近一次 curator 运行结果

**3.5 Curator 分类审计**

- **目标**：实现 post-hoc 分类（consolidated vs pruned）
- **依赖**：3.3 LLM pass 完成后
- **修改文件**：
  - `loom/src/background_review/curator.rs` — 新增 `classify_removed_skills` + `reconcile_classification`
- **验收**：Curator 报告中区分 "合并到 X" 和 "已归档" 的技能

### Phase 4：架构补齐（持续）

> 补齐 Hermes 有但 Loom 缺失的辅助功能，按需实施。

**4.1 Curator 快照与回滚**

- **目标**：curator 运行前自动快照，支持回滚
- **实现**：tar.gz 打包 `~/.loom/skills/` → `~/.loom/data/curator/backups/`
- **修改文件**：新增 `loom/src/background_review/curator_backup.rs`

**4.2 Prefix Cache 复用**

- **目标**：Review agent 复用主 agent 的 system prompt prefix cache
- **前提**：需要 LLM provider 支持 prompt caching（OpenAI automatic caching）
- **修改文件**：
  - `loom/src/background_review/agent_loop.rs` — `build_system_prompt` 改为复用主 agent 的 prefix

**4.3 Memory write origin/context 标记**

- **目标**：memory 写入时标记来源为 `background_review`
- **修改文件**：
  - `loom/src/background_review/tools.rs` — `memory_set` 操作增加 origin 字段
  - `loom/src/background_review/memory.rs` — memory 记录增加 origin/context 字段

**4.4 Skill preprocessing**

- **目标**：SKILL.md 模板变量替换和 inline shell 展开
- **修改文件**：新增 `loom/src/background_review/skill_preprocessing.rs`

**4.5 Skill bundles**

- **目标**：多技能批量加载别名
- **修改文件**：新增 `loom/src/background_review/skill_bundles.rs`

**4.6 工具护栏 (Tool Guardrails)**

- **目标**：无副作用决策引擎，防止工具调用循环
- **修改文件**：新增 `loom/src/tools/guardrails.rs`

---

### 实施优先级总览

```
Phase 1 (1-2天)  ████████████ 正确性修复
Phase 2 (2-3天)  ██████████████████ Prompt补全 + 效率优化
Phase 3 (1-2周)  ████████████████████████████████ Curator增强
Phase 4 (持续)   ████████████████████████████████████████ 架构补齐
```

| Phase | 项目数 | 预计工期 | 核心交付物 |
|-------|--------|----------|-----------|
| Phase 1 | 4 | 1-2 天 | 完整会话历史传入、never-used bug 修复、Reactivation、Protected skills |
| Phase 2 | 4 | 2-3 天 | SKILL/COMBINED prompt 补全、nudge interval、实际 tier 解析 |
| Phase 3 | 5 | 1-2 周 | LLM consolidation、报告系统、分类审计、空闲触发、状态管理 |
| Phase 4 | 6 | 持续 | 快照回滚、prefix cache、memory origin、skill preprocessing、bundles、guardrails |

---

## 附录 A：源码验证记录（2026-05-26）

以下事实通过直接阅读源码验证：

### Hermes 源码行数

| 文件 | 声明行数 | 实际行数 |
|------|---------|---------|
| `agent/curator.py` | 1781 | **1781** ✅ |
| `agent/curator_backup.py` | — | **693** |
| `agent/background_review.py` | — | **582** |

### Loom 源码行数与结构

| 文件 | 声明 | 实际 |
|------|------|------|
| `cli/src/run/curator.rs` | 336行 | **2行（re-export）** ❌ 原文有误 |
| `cli/src/run/review_prompts.rs` | 独立实现 | **2行（re-export）** ❌ 原文有误 |
| `cli/src/run/review_tools.rs` | 独立实现 | **2行（re-export）** ❌ 原文有误 |
| `cli/src/run/review_agent_loop.rs` | 独立实现 | **2行（re-export）** ❌ 原文有误 |
| `cli/src/run/background_review.rs` | — | **22行（唯一非 re-export 入口）** |
| `cli/src/run/evolution_trigger.rs` | — | **2行（re-export）** |
| `cli/src/review_history.rs` | — | **2行（re-export）** |
| `loom/src/background_review/curator.rs` | — | **335行** |
| `loom/src/background_review/prompts.rs` | — | **217行** |
| `loom/src/background_review/tools.rs` | — | **428行** |
| `loom/src/background_review/agent_loop.rs` | — | **206行** |
| `loom/src/background_review/workflow.rs` | — | **359行** |
| `loom/src/background_review/evolution.rs` | — | **32行（仅 config 类型）** |

**关键发现**：`cli/src/run/` 下的 review/curator/evolution 文件均为 2 行 re-export（`pub use loom::background_review::*`），不存在"双重实现"。唯一非 re-export 的是 `cli/src/run/background_review.rs`（22行入口）。

### Hermes 触发机制验证

- **Memory nudge**: `agent_init.py` L961 默认 `_memory_nudge_interval = 10`（turn-based）
- **Skill nudge**: `agent_init.py` L1068 默认 `_skill_nudge_interval = 10`（iteration-based）
- **触发位置**: `conversation_loop.py` L388-394 (memory) + L4047-4051 (skill)
- **Review fork**: `_run_review_in_thread()` 在 `background_review.py` L321，fork 的 AIAgent 继承父 agent 的 runtime/credentials
- **工具白名单**: `set_thread_tool_whitelist(review_whitelist)` 限制为 memory+skills toolset
- **max_iterations**: L395 `max_iterations=16`

### Loom 触发机制验证

- **默认配置**: `BackgroundReviewConfig::default()` — `max_iterations: 16`, `max_session_chars: 24000`
- **触发**: `spawn_background_review()` → `tokio::spawn` → `run_background_review_workflow()`
- **Curator 自动运行**: `run_curator_if_needed()` 基于 `state.json` 修改时间间隔（默认 86400 秒）
- **工具**: 10 个（memory_get/set, skills_list, skill_view/create/edit/patch/delete, skill_write_file/remove_file）

### Hermes curator_backup.py 验证

- **快照**: `snapshot_skills()` 将 skills 目录打包为 tar.gz，同时备份 `cron/jobs.json`
- **排除**: `.curator_backups/`（避免递归）、`.hub/`（由 hub 管理）
- **回滚**: `rollback_to_snapshot()` 恢复快照，当前 tree 移到新快照（回滚可撤销）
- **配置**: `DEFAULT_KEEP = 5`

### CURATOR_REVIEW_PROMPT 行数

- **声明**: 114 行
- **实际**: ~115 行（L330-L444，含赋值语句）
- **判定**: ✅ 基本准确


