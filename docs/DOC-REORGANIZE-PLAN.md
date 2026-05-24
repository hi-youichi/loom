# 文档整理方案

> 状态: Draft
> 日期: 2025-08-19
> 分支: dev

## 目标

将 `docs/` 下散落的文件按功能分类归档，建立清晰的目录结构。

## 目标结构

```
docs/
├── guide/                # 功能指南
├── dev/
│   ├── design/           # 设计文档
│   ├── impl/             # 实现计划
│   └── evolution/        # 进化系统（原 evolution/）
├── rfc/                  # 提案（原 rfcs/）
├── misc/                 # 杂项
├── getting-started/      # 保持不变
├── core/                 # 保持不变
├── advanced/             # 保持不变
├── reference/            # 保持不变
├── deployment/           # 保持不变
├── streaming/            # 保持不变
├── memory/               # 保持不变
├── adr/                  # 保持不变
└── README.md             # 重写为新结构索引
```

## 执行顺序

1. 创建目标目录：`dev/design/`、`dev/impl/`、`misc/`
2. 按分类移动文件（见下方清单）
3. 修复文档间的内部链接
4. 删除移空的目录：`design/`、`dev-plan/`、`tools/`
5. 重写 `docs/README.md` 为新结构索引
6. 删除 `docs/DOC-PLAN.md`

## 提交策略

按分类分 6 个 commit：
1. `docs: 整理 guide/ 功能指南`
2. `docs: 整理 dev/design/ 设计文档`
3. `docs: 整理 dev/impl/ 实现计划`
4. `docs: 迁移 evolution/ → dev/evolution/`
5. `docs: 重命名 rfcs/ → rfc/ 并补充提案`
6. `docs: 整理 misc/ + 重写索引 + 清理空目录`

## 详细移动清单

### 1. guide/ — 功能指南

| 源 | 目标 |
|----|------|
| `docs/hide-executing-tool-guide.md` | `docs/guide/hide-executing-tool.md` |
| `docs/chatgpt-prompt-engineering.md` | `docs/guide/prompt-engineering.md` |
| `docs/tools/telegram.md` | `docs/guide/telegram.md` |
| `docs/tools/codex-shell-execution.md` | `docs/guide/codex-shell-execution.md` |
| `docs/tools/shell-background-timeout.md` | `docs/guide/shell-background-timeout.md` |
| `docs/writing-good-docs.md` | `docs/guide/writing-docs.md` |
| `docs/skills.md` | `docs/guide/skills.md` |

### 2. dev/design/ — 设计文档

| 源 | 目标 |
|----|------|
| `docs/llm-tool-design.md` | `docs/dev/design/llm-tool.md` |
| `docs/tool-display-ux.md` | `docs/dev/design/tool-display-ux.md` |
| `docs/codex-goal-feature.md` | `docs/dev/design/codex-goal.md` |
| `docs/codex-goal-source-analysis.md` | `docs/dev/design/codex-goal-source-analysis.md` |
| `docs/design/tool-display-ux-improvement.zh.md` | `docs/dev/design/tool-display-ux-improvement.zh.md` |
| `docs/design/goal-external-loop.md` | `docs/dev/design/goal-external-loop.md` |
| `docs/design/goal-ralph-loop.md` | `docs/dev/design/goal-ralph-loop.md` |
| `docs/design/session-dump.md` | `docs/dev/design/session-dump.md` |
| `docs/design/session-lifecycle.md` | `docs/dev/design/session-lifecycle.md` |
| `docs/design/session-cat-tasks.md` | `docs/dev/design/session-cat-tasks.md` |
| `docs/design/acp-goal-support.md` | `docs/dev/design/acp-goal-support.md` |
| `docs/design/architecture.md` | `docs/dev/design/architecture.md` |
| `docs/design/claude-code-compat.md` | `docs/dev/design/claude-code-compat.md` |
| `docs/design/data-format.md` | `docs/dev/design/data-format.md` |
| `docs/design/decisions.md` | `docs/dev/design/decisions.md` |
| `docs/design/meta-agent-architecture.md` | `docs/dev/design/meta-agent-architecture.md` |
| `docs/design/task-integration.md` | `docs/dev/design/task-integration.md` |
| `docs/tools/tool-system.md` | `docs/dev/design/tool-system.md` |
| `docs/tools/mcp.md` | `docs/dev/design/mcp.md` |
| `docs/dev/act-node-architecture.md` | `docs/dev/design/act-node-architecture.md` |
| `docs/dev/backend-trait.md` | `docs/dev/design/backend-trait.md` |
| `docs/dev/logging-system-refactoring.md` | `docs/dev/design/logging-system-refactoring.md` |
| `docs/dev/tech-stack.md` | `docs/dev/design/tech-stack.md` |
| `HERMES_ANALYSIS_AND_LOOM_PLAN.md` | `docs/dev/design/hermes-analysis.md` |

### 3. dev/impl/ — 实现计划

| 源 | 目标 |
|----|------|
| `docs/llm-tool-dev-plan.md` | `docs/dev/impl/llm-tool.md` |
| `docs/cli-ux-improvement-plan.md` | `docs/dev/impl/cli-ux-improvement.md` |
| `docs/cli-ux-improvement-plan.zh.md` | `docs/dev/impl/cli-ux-improvement.zh.md` |
| `dev-plan-goal-runner-event-output.md` | `docs/dev/impl/goal-runner-event-output.md` |
| `docs/dev-plan/file-change-tracking.md` | `docs/dev/impl/file-change-tracking.md` |
| `docs/design/goal-external-loop-dev-plan.md` | `docs/dev/impl/goal-external-loop.md` |
| `docs/design/goal-external-loop-task-list.md` | `docs/dev/impl/goal-external-loop-task-list.md` |
| `docs/dev/roadmap.md` | `docs/dev/impl/roadmap.md` |
| `docs/dev/nextest-guide.md` | `docs/dev/impl/nextest-guide.md` |

### 4. dev/evolution/ — 进化系统

| 源 | 目标 |
|----|------|
| `docs/evolution/` (整个目录) | `docs/dev/evolution/` |

> 整体迁移，内部 18 个文件保持不动。包含 review、curator、skills、memory 等进化子系统设计与实现。

### 5. rfc/ — 提案

| 源 | 目标 |
|----|------|
| `docs/rfcs/` (整个目录) | `docs/rfc/` |
| `docs/rfc-slash-command-registry.md` | `docs/rfc/slash-command-registry.md` |
| `docs/tool-display-ux-proposal.md` | `docs/rfc/tool-display-ux-proposal.md` |

> 注：`rfcs/` → `rfc/` 重命名可能影响外部引用，移动后需全局搜索更新路径。

### 6. misc/ — 杂项

| 源 | 目标 |
|----|------|
| `docs/plan-browser-extension.md` | `docs/misc/browser-extension.md` |
| `docs/codex_json_output_documentation.md` | `docs/misc/codex-json-output.md` |
| `LEVOL_DESIGN.md` | `docs/misc/levol-design.md` |
| `loom-acp/docs/ACP_COMMAND_GUIDE.md` | `docs/misc/acp-command-guide.md` |
| `loom-acp/docs/ACP_IMPLEMENTATION.md` | `docs/misc/acp-implementation.md` |

### 7. bugs/ — Bug 记录

| 源 | 目标 |
|----|------|
| `docs/openai-compat-fix.md` | `docs/bugs/openai-compat-fix.md` |
| `docs/dev/acp/incident-2026-04-28-abnormal-exit.md` | `docs/bugs/acp-abnormal-exit-2026-04-28.md` |

### 8. 清理

| 文件 | 操作 |
|------|------|
| `docs/DOC-PLAN.md` | 整理完成后删除 |
| `docs/README.md` | 重写为新结构索引 |
| `docs/design/` | 移空后删除目录 |
| `docs/dev-plan/` | 移空后删除目录 |
| `docs/tools/` | 移空后删除目录 |
| `docs/evolution/` | 移走后删除目录 |
| `Dockerfile.crypto` | 保留在根目录（非文档） |

### 9. 不动的已有目录和文件

- `docs/getting-started/`
- `docs/core/`
- `docs/advanced/`
- `docs/reference/`
- `docs/deployment/`
- `docs/streaming/`
- `docs/memory/`
- `docs/adr/`
- `docs/guide/` (现有文件保留)
- `docs/dev/acp/` (移走 incident 后剩余文件保留)
- `docs/dev/models.dev/`
- `docs/dev/openrouter/`
- `docs/diagnostics/`
- `docs/reviews/`
- `docs/bugs/` (现有文件保留)

## 不在本次范围内的文件

- `penpot-reference` — 子模块，不动
- `web/website/docusaurus.config.ts` + `sidebars.ts` — 网站配置，属于代码变更
