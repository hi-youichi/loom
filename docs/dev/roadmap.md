# Loom 开发路线图

> **状态**：活跃
> **最后更新**：2026-06-21
> **来源**：汇总 `docs/dev/plans/` 待办 plan + analysis/review 文档
>
> **关联路线图**：
> - 进化子系统（Phase 2-9：后台审查、技能系统、Curator、GEPA）：[docs/evolution/roadmap.md](../evolution/roadmap.md)
> - Hermes 对齐专题：[docs/dev/loom-hermes-alignment/](loom-hermes-alignment/)
>
> **文档约定**：遵循 [开发文档编写指南](guide/writing-dev-docs.md)。编号 `#NN` 为稳定标识符，不重排。

---

## 1. 概述

本路线图索引全部待办开发项，按优先级（P1-P4）组织。每项关联对应的 plan 文档（`plans/`）或分析文档（`analysis/`、`loom-hermes-alignment/`）。已完成项见文末 [§5 已完成归档](#5-已完成归档)。

**当前进度**：P0 4/4 ✅；P1 8/8 ✅；P2 22/22 ✅；P3 已完成 5/7（#35, #36, #38, #39, #40），待办 #31/#32 (config) / #33/#34 (god crate) / #47 (agent API)；P4 已完成 5/5（#37-partial, #38, #39, #40, #42）。

---

## 2. 待办任务

### P2 — 功能对齐（~6d / 45h）

> 与 Hermes 能力对齐 + Memory 子系统增强。多数任务以小时计。
>
> **✅ 全部已完成** — 详见 [§5 已完成归档](#p2--已完成项-1)。

| # | 任务 | 工时 | Plan / 来源 | 依赖 |
|---|------|------|-------------|------|
| 12 | **Memory：条目分隔符 `§` + 解析 + 旧格式迁移** | 2h | [006](plans/006-loom-memory-enhancement.md) P0-a | — |
| 13 | **Memory：`add_entry` / `replace_entry` / `remove_entry` + 去重** | 4h | [006](plans/006-loom-memory-enhancement.md) P0-b | #12 |
| 14 | **Memory：原子写入（tempfile + fsync + rename）** | 1h | [006](plans/006-loom-memory-enhancement.md) P0-c | #12 |
| 15 | **Memory：冻结快照（`capture_snapshot` + prefix cache 稳定）** | 2h | [006](plans/006-loom-memory-enhancement.md) P0-d | #12 |
| 16 | **Memory：FACTS.md → PROJECT.md 迁移 + `MemoryFile` 枚举** | 2h | [006](plans/006-loom-memory-enhancement.md) P0-e | #12 |
| 17 | **Memory：`memory` 工具实现 + 注册 + 弃用旧 4 工具** | 3h | [006](plans/006-loom-memory-enhancement.md) P0-f | #12-16 |
| 18 | **Memory：文件锁（`fs4` crate）** | 1h | [006](plans/006-loom-memory-enhancement.md) P1-a | #12 |
| 19 | **Memory：容量预算检查 + usage 格式化** | 1h | [006](plans/006-loom-memory-enhancement.md) P1-b | #12 |
| 20 | **Memory：漂移检测 + 备份** | 2h | [006](plans/006-loom-memory-enhancement.md) P1-c | #12 |
| 21 | **Review prompt 对齐 Hermes**（简化版 → 完整版） | 2-3h | [012](plans/012-background-review-alignment.md) #1 | — |
| 23 | **Tool whitelist 隔离**（限制 review agent 可用工具集） | 2h | [012](plans/012-background-review-alignment.md) #2 | #22 |
| 24 | **记忆写入 provenance**（区分后台/前台写入） | 1-2h | [012](plans/012-background-review-alignment.md) #3 | — |
| 25 | **Curator LLM Pass 完整链路**（`build_llm_prompt()` → 调用入口） | 3-4h | [013](plans/013-curator-alignment.md) #1 | — |
| 26 | **Curator prompt 补全**（55 行简化版 → 完整版） | 2h | [013](plans/013-curator-alignment.md) #2 | #25 |
| 27 | **Per-run 报告持久化**（`CuratorRunReport` 写入） | 2h | [013](plans/013-curator-alignment.md) #3 | #25 |
| 28 | ~~**`absorbed_into` 声明**（函数已有，prompt 未要求）~~ ✅ | ~~0.5h~~ | [013](plans/013-curator-alignment.md) #4 | — |
| 29 | **Protected skills**（基于 `SkillMeta.pinned` 实现删除保护） | 2h | [014](plans/014-shared-infra-alignment.md) #1 | — |
| 30 | **包完整性检查**（Curator 归档前验证 skill 包完整性） | 2h | [013](plans/013-curator-alignment.md) #5 | — |
| 46 | **`session list` 分页浏览**（类似 `git log`，支持 `--page`/`--limit`） | 2-3h | [009-session-list-pagination.md](plans/009-session-list-pagination.md) | — |

### P3 — 重构优化（~18-29d）

> 架构层面的深度重构，改善编译时间和可维护性。

| # | 任务 | 工时 | Plan / 来源 | 依赖 |
|---|------|------|-------------|------|
| 31 | **新增 `config/src/constants.rs`**（591 处 `env::var` 字面量 → `pub const`） | 5-7d | [015](plans/015-config-centralization.md) #1-2 | — |
| 32 | **`loom-react-config::from_env()` 改用 `config` crate**（27 处 env::var） | 1d | [015](plans/015-config-centralization.md) #3 | #31 |
| 33 | **`loom` Cargo.toml 瘦身**（30 path 依赖 → thin facade） | 5-10d | [016](plans/016-god-crate-split.md) #1-2 | — |
| 34 | **`agent-extensions` 拆分**（20 内部依赖 → 3-4 小 crate） | 3-5d | [016](plans/016-god-crate-split.md) #3-5 | — |
| 35 | ~~**Memory 内容安全扫描**（`threat_patterns` 模块）~~ ✅ | ~~3h~~ | [006](plans/006-loom-memory-enhancement.md) P2 | — |
| 36 | ~~**Context fencing**（`<memory-context>` 标签 + `StreamingContextScrubber`）~~ ✅ | ~~3h~~ | [006](plans/006-loom-memory-enhancement.md) P3 | — |
| 47 | **Agent 高层 API**（streaming-only、profile 优先、零暴露内部类型） | 3-5d | [001-agent-api-design.md](plans/001-agent-api-design.md) | — |

### P4 — 长期演进（~10-18d）

> 技术债清理，可穿插在功能开发间隙逐步推进。全部独立。

| # | 任务 | 工时 | Plan / 来源 |
|---|------|------|-------------|
| 37 | ~~**死代码全量清理**（275 处 `#[allow(dead_code)]`）~~ ✅ partial | ~~5-10d~~ | [017](plans/017-dead-code-cleanup.md) |
| 38 | ~~**错误类型架构重构**（拆分 `AgentError` → `GraphError` + `LlmError`）~~ ✅ | ~~1-2d~~ | [004-error-type-architecture.md](plans/004-error-type-architecture.md) |
| 39 | ~~**`openai_compat.rs` clone 优化**（streaming hot path: 6 clones/delta → 0）~~ ✅ | ~~1-2d~~ | [018](plans/018-llm-hot-path-optimization.md) |
| 40 | ~~**菱形依赖消除**（`loom ↔ loom-react-config ↔ agent` 循环）~~ ✅ | ~~2-3d~~ | [019](plans/019-diamond-dependency.md) |
| 42 | ~~**Curator 首次运行延迟**（避免启动时立即触发）~~ ✅ | ~~0.5d~~ | [013](plans/013-curator-alignment.md) #6 |

> #41 (Aux model) 已完成，归档至 [§5 P2 已完成项](#p2--已完成项-1)。

---

## 3. 依赖关系

```
P1  #22 Review Fork ──── 独立
    #43 CLI 去重 ──────── 独立
    #44 Review 路径统一 ── 依赖 #22

P2  #12-20 Memory ────── 内部有序（#12 → #13-20）
    #21/#23/#24 BG Review ── 独立
    #25-28 Curator ────── #25 优先（LLM Pass 是其他项前提）
    #29-30 共享基础设施 ── 独立
    #46 session 分页 ──── 独立

P3  #31 → #32 配置集中化
    #33/#34 独立
    #35/#36 依赖 P2 Memory 完成
    #47 独立（ReactBuildConfig 已稳定 ✅）

P4  全部独立
```

---

## 4. 暂缓（不实施）

| 项目 | 原因 | 来源 |
|------|------|------|
| Prefix cache 复用 | ROI 低，需侵入 `loom-llm`；Hermes 实测 ~26% token 节省但实现复杂 | [03-shared-infra.md](loom-hermes-alignment/03-shared-infra.md) §1 |
| Cron 引用重写 | Loom 无 cron job 系统 | [02-curator.md](loom-hermes-alignment/02-curator.md) §3.3 |
| `agent-core` 盲目拆分 | 核心枢纽角色固有高依赖，应做依赖注入而非物理拆分 | [architecture-audit.md](analysis/architecture-audit.md) P0-2 |
| 缓存失效（Skill 两层 LRU + 磁盘快照） | 随缓存功能整体设计后再实施 | [010-skill-system-alignment-plan.md](archive/plans/010-skill-system-alignment-plan.md) |

---

## 5. 已完成归档

> 以下内容已从路线图主体移除，记录于此供追溯。

### P0 — 紧急修复（✅ 全部完成）

| # | 任务 | 文件 |
|---|------|------|
| 1 | 修复 `COMPAT_RETRY_MAX_RETRIES = 999` → 20 次 | `loom-llm/src/client/openai_compat.rs` |
| 2 | 删除 `tool_source.rs` 死代码块（~170 行） | `agent/agent-core/src/agent/react/build/tool_source.rs` |
| 3 | `think_node.rs` `RwLock<HashMap>` → `DashMap` | `think_node.rs` |
| 4 | 移除 `hang_probe` 调试日志（4 文件，~42 处） | `runner_common.rs` + `think_node.rs` + `openai_compat.rs` + `traits.rs` |

### P1 — 已完成项

| # | 任务 | 来源 |
|---|------|------|
| 6 | `invoke_agent.rs` 提取 `build_and_run_sub_agent()`（~350 行去重） | [agent-core-code-review.md](review/agent-core-code-review.md) P1-2 |
| 7 | `run_stdio_loop` 拆分（240 行 → 3 函数） | [architecture-audit.md](analysis/architecture-audit.md) D7 |
| 8 | `openai_compat.rs` 文件拆分（1438 行 → 4 模块） | [architecture-audit.md](analysis/architecture-audit.md) D8 |
| 9-10 | `loom-helve` 代号消除 + 层级扁平化 | [005-helve-flatten-redesign.md](archive/plans/005-helve-flatten-redesign.md) ✅ |
| 11 | `.loom/skills/` 扁平 → 分类目录（214 skills → 13 分类） | [010-skill-system-alignment-plan.md](archive/plans/010-skill-system-alignment-plan.md) ✅ |
| 22 | **Review Agent Fork 机制**（`run_with_config` + `ReviewToolGate` + `review.rs`） | [008](plans/008-review-agent-fork.md) ✅ |
| 43 | **CLI agent 流显示去重**（`loom-stream-display` 统一） | [002-cli-agent-dedup.md](plans/002-cli-agent-dedup.md) ✅ |
| 44 | **Review 路径统一**（删除 Path A，保留 Path B `workflow.rs`） | [003-delete-path-a-keep-path-b.md](plans/003-delete-path-a-keep-path-b.md) ✅ |

### P2 — 已完成项

| # | 任务 | 来源 |
|---|------|------|
| 12-20 | **Memory 子系统增强**（分隔符 § + add/replace/remove + 原子写入 + 冻结快照 + PROJECT.md 枚举 + memory 工具 + 文件锁 + 容量预算 + 漂移检测） | [006](plans/006-loom-memory-enhancement.md) ✅ `experimental/memory-v2` |
| 21 | **Review prompt 对齐 Hermes**（完整版，含审查准则 + 行动指南） | [012](plans/012-background-review-alignment.md) #1 ✅ |
| 23 | **Tool whitelist 隔离**（`ReviewToolGate` 白名单） | [012](plans/012-background-review-alignment.md) #2 ✅ |
| 24 | **记忆写入 provenance**（`MemoryProvenance` + `WriteOrigin` 区分前后台） | [012](plans/012-background-review-alignment.md) #3 ✅ |
| 25 | **Curator LLM Pass 完整链路**（`run_curator_llm_pass` + `run_curator_llm_if_needed`） | [013](plans/013-curator-alignment.md) #1 ✅ |
| 26 | **Curator prompt 补全**（`CURATOR_SYSTEM_PROMPT` 完整版） | [013](plans/013-curator-alignment.md) #2 ✅ |
| 27 | **Per-run 报告持久化**（`CuratorRunReport::save_to_dir`，CLI + BG 路径） | [013](plans/013-curator-alignment.md) #3 ✅ |
| 28 | **`absorbed_into` 声明**（`extract_absorbed_into_declarations` + `forget_with_intent`） | [013](plans/013-curator-alignment.md) #4 ✅ |
| 29 | **Protected skills**（pinned skills 不可删除/归档，`SkillError::Pinned`） | [014](plans/014-shared-infra-alignment.md) #1 ✅ |
| 30 | **包完整性检查**（Curator 归档前验证） | [013](plans/013-curator-alignment.md) #5 ✅ |
| 41 | **Aux model 配置**（`LOOM_AUX_MODEL` for review/curator） | [014](plans/014-shared-infra-alignment.md) #2 ✅ |
| 45 | **Review Agent 记忆配置字段对齐**（7 字段） | [011 系列](archive/plans/011-review-agent-memory-config.md) ✅（2025-07） |
| 46 | **`session list` 分页浏览**（`list_sessions_filtered` + limit） | [009-session-list-pagination.md](plans/009-session-list-pagination.md) ✅ |

### 已废弃 Plan

| Plan | 原因 |
|------|------|
| [007](archive/plans/007-rename-llm-error.md) | 被 [004](plans/004-error-type-architecture.md) 取代（扩展为完整错误类型层级重构） |

---

## 6. 核心风险

| 风险 | 影响范围 | 缓解 |
|------|---------|------|
| 底层 CLI 输出格式变更 | 输出处理链路 | Backend Adapter 隔离；PTY 兜底 |
| Context 文件注入冲突 | Memory 子系统 | 标记区间 + 原子还原（#14） |
| **#33 God Crate 拆分引入循环依赖** | 全 workspace 编译 | 拆分前绘制依赖图；分阶段提交 + 独立编译验证 |
| **#34 agent-extensions 拆分后接口稳定性** | 8+ 内部调用方 | 先抽象 trait 边界，再物理拆分；提供 deprecation alias |
| **进化子系统（GEPA）尚未实现** | evolution Phase 6-8 不可执行 | 进化路线图已标注规划阶段，dev/roadmap 不依赖其完成 |

> 进化子系统的专属风险见 [docs/evolution/roadmap.md](../evolution/roadmap.md) §核心风险。

---

## 历史修订

- **2025-08-19**：为全部缺少 plan 的任务创建 plan 文档——新增 [012](plans/012-background-review-alignment.md)（#21/#23/#24）、[013](plans/013-curator-alignment.md)（#25-28/#30/#42）、[014](plans/014-shared-infra-alignment.md)（#29/#41）、[015](plans/015-config-centralization.md)（#31/#32）、[016](plans/016-god-crate-split.md)（#33/#34）、[017](plans/017-dead-code-cleanup.md)（#37）、[018](plans/018-llm-hot-path-optimization.md)（#39）、[019](plans/019-diamond-dependency.md)（#40）；roadmap 所有任务现引用 `plans/` 文档。
- **2025-08-19**：按开发指南重组——合并 Plans 总览与 P1-P4 任务表为统一格式（`# / 任务 / 工时 / Plan / 依赖`）；移除冗余的工时总览表和 Sprint 排期（与优先级分组重复）；依赖图简化为单行格式；核心风险精简为 5 项。
- **2025-08-19**：重组为纯待办路线图——删除全部已完成内容（P0 #1-#4、P1 #6-#11）、已实现 plan（005/010）、已废弃 plan（007），移入「已完成归档」；新增 #43-#47；新增「Plans 文档总览」section。
- **2026-06-21**：修正 P0 状态（5/5 → 4/4）、P1/P2/P3/P4 工时对齐、补全 #22 工时（2-3h → 6h）、增加 evolution roadmap 交叉引用。
- **2025-08-19**：初始版本（4 P0 + 7 P1 + 18 P2 + 6 P3 + 6 P4 = 41 任务）。
