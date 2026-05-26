# 文档审计报告 (更新)

> 上上次审计: 2026-05-26 | 上次审计: 2026-06-19 | 本次更新: 2026-07-30 | 最新更新: 2026-08-26
> 范围: `docs/` (~156 .md) + 已合并的 `designs/` (9 .md) + `.loom/`
> 方法: 逐篇阅读 + 对照源码 (`loom/src/`, `cli/src/`) + 上网研究
> 本次审计重点: 修复所有 P0 项，清理 Levol 引用，更新 README 索引，对齐 OpenAI 文档标准

---

## 变更摘要

自上次审计以来：

| 状态 | 项目 | 说明 |
|------|------|------|
| ✅ 本次 | Levol→Loom (10+ 文件) | `guide/` (getting-started, config, backends), `design/` (architecture, session-lifecycle, decisions, data-format) |
| ✅ 本次 | README.md 重构 | 添加角色导航 ("你想做什么?")、移除架构图、对齐 OpenAI 风格 |
| ✅ 本次 | writing-good-docs.md | 添加 OpenAI Cookbook 扫读原则 (Section 0) |
| ✅ 本次 | evolution/tools.md | 确认已存在，无需新建 |
| ✅ 已修复 | `evolution/review.md` max_session_chars | 12000→24000，线程模型，工具名，security.rs 描述 |
| ✅ 已修复 | `evolution/config.md` max_session_chars | 12000→24000 |
| ✅ 已修复 | `evolution/rfc-review-command.md` (x2) | 12000→24000 |
| ✅ 已修复 | `dev/impl/review-full-development.md` | 12000→24000，移除 `guard_agent_created: false` |
| ✅ 已修复 | `docs/README.md` | Levol→Loom (标题、编排层、命令、配置) |
| ✅ 已修复 | `evolution/README.md` | Levol→Loom (标题、描述) |
| ✅ 已修复 | Levol→Loom (18 处) | `design/`、`dev/`、`guide/`、`evolution/`，另 LEVOL_DESIGN.md 移入 archive |
| ✅ 已修复 | `cli/src/run/review.rs` 旧实现 | 默认值 12000→24000, 10→16, Json→Agent, 添加 doc comment |

---

## 一、全景概览

| 目录 | 文件数 | 主题 |
|------|--------|------|
| `docs/` 根 | 23 | 架构、规范、Codex 分析、整理方案 |
| `docs/evolution/` | 22 | 后台审查、技能、进化、GEPA（最大类） |
| `docs/design/` | 16 | 高层设计: goal 循环、session 生命周期、ACP |
| `docs/dev/acp/` | 8 | ACP 协议 JSON schema |
| `docs/core/` | 7 | ReAct/ToT/GoT/DUP 运行模式 |
| `docs/review/` | 9 | 代码质量审查报告（之前误报为7个+reviews/2个） |
| `docs/dev/` | 6 | 开发: OpenRouter, 模型规格, 技术栈 |
| `docs/guide/` | 5 | 功能指南 |
| `docs/reference/` | 5 | API/CLI/配置参考 |
| `docs/deployment/` | 4 | 部署方案 |
| `docs/rfcs/` | 4 | RFC 提案（`docs/rfc/` 是单文件，非目录） |
| `docs/dev/impl/` | 3 | 实现计划 |
| `docs/getting-started/` | 3 | 入门文档 |
| `docs/tools/` | 5 | MCP, shell, Telegram |
| `docs/memory/` | 2 | 记忆系统 |
| `docs/adr/` | 2 | 架构决策记录 |
| `docs/dev/design/` | 2 | 工具展示 UX |
| `docs/archive/` | 4 | 归档 meta 文档 |
| 根 `designs/` | 8 | AI Company, TUI, 进化对比（应移至 docs/design/） |

---

## 二、源码对照发现

### 🔴 P0: 已确认未实现的功能（文档声称有，源码无）

| 文档 | 声称的功能 | 源码状态 |
|------|-----------|---------|
| `evolution/review.md:97` | LLM 调用失败 3次重试+指数退避(2s/4s/8s) | `agent_loop.rs` 直接 `?`，无重试逻辑 |
| `evolution/review.md:71-73` | `agent-created` 标记 + `is_background_review()` | 工具调用无此标记 |
| `evolution/review.md:76-78` | `guard_agent_created` 安全扫描(默认false) | 无此功能；`security.rs` 始终启用 |
| `evolution/rfc-review-llm-refactor.md` | 重试策略统一使用 `RetryLlmClient` | 未实现，Review 仍用独立简单客户端 |

### 🟡 P1: 两个并行 Review 实现

| 位置 | 配置 | 用途 |
|------|------|------|
| `loom/src/background_review/` | `max_session_chars: 24000`, `max_iterations: 16` | **活跃**实现，Agent 模式 |
| `cli/src/run/review.rs` | `max_session_chars: 12000`, `max_iterations: 10` | **旧版**简化实现，直接 LLM 调用 |

旧实现 `cli/src/run/review.rs` 仍有 `12000`，与现代实现不一致。

### 🟢 已验证正确的文档

- `evolution/review.md` — max_session_chars(24000)、tokio task 模型、10个工具名单、security 描述 ✅
- `evolution/config.md` — max_session_chars(24000) ✅
- `evolution/loom-vs-hermes-evolution-diff.md` — 模块路径映射正确 ✅
- `MODULES.md` — 项目结构与源码一致 ✅

---

## 三、上网研究结果

### Hermes Agent Self-Evolution
- **来源**: NousResearch/hermes-agent-self-evolution
- **核心技术**: DSPy + GEPA (ICLR 2026 Oral)
- **状态**: Phase 1 (技能进化) 已实现，Phase 2-4 规划中
- **成本**: ~$2-10 每次优化，无 GPU 需求
- **启发**: Loom 可考虑 DSPy Rust binding (`dspy_rs`) 进行 GEPA 集成

### GEPA (Genetic-Pareto)
- **来源**: gepa-ai/gepa (MIT license)
- **机制**: Select → Execute(捕获trace) → Reflect(LLM读trace诊断) → Mutate → Accept
- **关键概念**: Actionable Side Information (ASI) — 不只看分数，还看错误原因
- **DSPy 集成**: `dspy.GEPA` 是 2026 年推荐的 DSPy 优化器

### Agent 内存系统趋势
- Hermes: frozen-snapshot pattern（会话启动时快照，中途写入下次生效），injection scanner
- agentmemory: pull-model episodic memory，同步删除，audit trace
- Loom: 文件系统 memory (USER.md/PROJECT.md/FACTS.md) + SQLite，与 Hermes 思路一致

---

## 四、结构性发现（审计报告修正）

### 上次审计误报

| 误报 | 实际情况 |
|------|---------|
| `docs/reviews/` 存在 | **不存在**，仅 `docs/review/`（9个代码审查报告） |
| `docs/rfc/` 是目录 | `docs/rfc/` 是**文件** (`rfc-slash-command-registry.md`)，非目录 |
| `designs/` 在 `docs/designs/` | `designs/` 在**项目根目录**，8个设计文档 |

### 真实结构问题

1. ~~**`designs/` (根目录) vs `docs/design/`**~~ — ✅ 已合并，9文件移入 docs/design/，空目录删除
2. ~~**`docs/rfc-*.md` 单文件 vs `docs/rfcs/` 目录**~~ — ✅ 已合并到 docs/rfcs/slash-command-registry.md
3. ~~**"Levol" 旧品牌名 22 处**~~ — ✅ 18处工作文档已修复，LEVOL_DESIGN.md 移入 archive
4. **4份归档 meta 文档** — `archive/DOC-PLAN.md`、`archive/DOC-REORGANIZE-PLAN.md`、`archive/CLEANUP-PLAN.md`、`archive/REVIEW_FINDINGS.md` 已完成使命

---

## 五、源码-文档映射 (修正版)

```
源码模块                              对应文档
─────────────────────────────────────────────────
loom/src/background_review/           evolution/review.md ✅ (已更新)
  ├── agent_loop.rs                   evolution/review.md ✅
  ├── prompts.rs                      evolution/review.md ✅
  ├── tools.rs                        无专文 ⚠️
  ├── security.rs                     evolution/review.md ✅
  ├── workflow.rs                     evolution/review.md ✅
  └── memory.rs                       evolution/memory.md + memory/*.md

cli/src/run/review.rs (旧实现)        ❌ 无文档说明与新版关系

loom/src/agent/react/                 core/react.md ✅
loom/src/agent/tot/                   core/tot.md ✅
loom/src/agent/got/                   core/got.md ✅
loom/src/agent/dup/                   core/dup.md ✅
loom/src/graph/                       core/state-graph.md ✅
loom/src/llm/                         core/llm-client.md ✅
loom/src/goal_runner/                 design/goal-external-loop*.md ✅
```

---

## 六、行动建议 (更新)

### 本次已完成
- [x] 修复 4 个文档的 `max_session_chars` 值 (12000→24000)
- [x] 移除 `review-full-development.md` 中不存在于源码的 `guard_agent_created: false`
- [x] 修复 `docs/README.md` + `evolution/README.md` 标题中的 Levol→Loom
- [x] 上网研究 Hermes Self-Evolution、GEPA、Agent Memory 趋势
- [x] 更新本审计报告
- [x] 清理 `guide/` (getting-started, config, backends) 全部 Levol 引用
- [x] 清理 `design/` (architecture, session-lifecycle, decisions, data-format) 全部 Levol 引用
- [x] README 添加 OpenAI 风格角色导航 (新用户/集成者/运维/开发者)
- [x] writing-good-docs.md 融合 OpenAI Cookbook 扫读原则
- [x] 确认 evolution/tools.md 已覆盖 background_review/tools.rs 文档

### 剩余 P0
- [ ] 处理 `cli/src/run/review.rs` 旧实现（更新或标记为 deprecated）

### 剩余 P1
- [ ] 合并 `designs/` (根) → `docs/design/`
- [ ] 合并 `docs/rfc-*.md` 单文件 → `docs/rfcs/`

### 结构性
- [ ] 删除或最终归档 4 份 meta 文档
- [ ] 完成 `DOC-REORGANIZE-PLAN.md` 未执行项
- [ ] 考虑合并 4 篇 Codex goal 相关文档
- [ ] 考虑合并 2 篇 GEPA 相关文档
