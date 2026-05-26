# CLI UX Documentation Index

> 本文档整理了 Loom CLI 用户体验相关的所有文档、设计文件及实现索引。

## 用户指南 (User Guides)

| 文档 | 位置 | 说明 |
|------|------|------|
| CLI 安装与配置 | `docs/deployment/cli.md` | 安装、配置、子命令参考、REPL、示例 |
| CLI JSON 流式输出 | `docs/deployment/cli-json-output.md` | `--json` 事件流协议与脚本集成 |
| session cat 会话回放 | `docs/deployment/cli-session-cat.md` | 会话回放 NDJSON 与文本摘要 |
| CLI 命令参考 | `docs/guide/cli.md` | 命令行快速参考 |
| 工具显示控制指南 | `docs/guide/hide-executing-tool.md` | Spinner 显示工具名/description 的方案 |

## 实现计划 (Implementation Plans)

| 文档 | 位置 | 说明 |
|------|------|------|
| CLI UX 改进方案 | `docs/dev/impl/cli-ux-improvement.md` | 6 阶段 CLI UX 改进计划 (Spinner/面板/Usage 等) |
| CLI UX 改进方案 (中文) | `docs/dev/impl/cli-ux-improvement.zh.md` | 同上，中文版 |
| Goal Runner 事件输出方案 | `docs/dev/impl/goal-runner-event-output.md` | 将 CLI 显示逻辑抽象到 loom crate 的计划 |
| 工具显示 UX 方案 | `docs/rfc/tool-display-ux-proposal.md` | CLI & Goal 工具显示 UX 优化提案 |

## 设计文档 (Design Docs)

| 文档 | 位置 | 说明 |
|------|------|------|
| 工具显示 UX 设计 | `docs/dev/design/tool-display-ux.md` | 工具显示交互体验问题与设计原则 |
| 工具显示 UX 改进 (中文) | `docs/dev/design/tool-display-ux-improvement.zh.md` | CLI & Goal 工具显示优化提案 v2 |

## 审查报告 (Reviews)

| 文档 | 位置 | 说明 |
|------|------|------|
| CLI UX 改进审查 | `docs/reviews/cli-ux-improvement-review.zh.md` | 对改进方案的 Review，含行业参考对比 |

## 核心实现文件 (Implementation)

| 文件 | 说明 |
|------|------|
| `cli/src/run/agent.rs` | CLI 流事件处理、stderr 显示回调、事件处理器 |
| `cli/src/run/display.rs` | 状态格式化与截断 |
| `cli/src/run/spinner.rs` | CLI Spinner 实现 |
| `cli/src/run/panel_format.rs` | CLI 结构化面板格式 |
| `cli/src/output.rs` | stdout/file 输出工具 (JSON & 文本) |
| `cli/src/repl.rs` | 交互式 REPL 循环 |
| `cli/src/display_limits.rs` | 截断常量 |
| `cli/src/args.rs` | CLI 参数定义 |
| `loom/src/stream_display/` | 共享显示模块 (format/spinner/panel_format/event_handler/markdown 等) |

## 迁移状态

- ✅ `docs/cli-ux-improvement-plan.md` → `docs/dev/impl/cli-ux-improvement.md`
- ✅ `docs/cli-ux-improvement-plan.zh.md` → `docs/dev/impl/cli-ux-improvement.zh.md`
- ✅ `dev-plan-goal-runner-event-output.md` → `docs/dev/impl/goal-runner-event-output.md`
- ✅ `docs/hide-executing-tool-guide.md` → `docs/guide/hide-executing-tool.md`
- ✅ `docs/tool-display-ux.md` → `docs/dev/design/tool-display-ux.md`
- ✅ `docs/tool-display-ux-proposal.md` → `docs/rfc/tool-display-ux-proposal.md`
- ✅ `docs/design/tool-display-ux-improvement.zh.md` → `docs/dev/design/tool-display-ux-improvement.zh.md`

## 文档与代码一致性审计 (2025-08-19)

| 文档 | 一致性状态 | 问题 |
|------|-----------|------|
| `docs/guide/cli.md` | ✅ 已修复 | 原引用 `levol chat` 完全过时，已重写为 `loom` 命令 |
| `docs/deployment/cli.md` | ⚠️ 部分修复 | 子命令表已补充，但 `--dry-run` 参数名已修正，仍需检查 agent 配置格式 |
| `docs/deployment/cli-json-output.md` | ⚠️ 标记待更新 | 描述 Codex CLI 协议而非 Loom 的 `ProtocolEvent`，顶部已加警告 |
| `docs/deployment/cli-session-cat.md` | ✅ 基本准确 | session cat 确实输出 CodexEvent，细节待确认 |
| `docs/guide/hide-executing-tool.md` | ✅ 准确 | 引用代码位置与实际一致 |
| `docs/review/03-cli-and-supporting.md` | ✅ 准确 | 代码审查文档，无过时内容 |

## CLI UX 实现进度 (vs improvement plan)

| 阶段 | 特性 | 状态 | 说明 |
|------|------|------|------|
| 1 | Spinner | ✅ 已实现 | spinner.rs: tick动画、TTY检测、pipe回退、NoopSpinner |
| 2 | 结构化面板 | ✅ 已实现 | panel_format.rs: CALL/DONE/USAGE/AGENT/TOOLS/MODEL |
| 3 | 思考/回复分离 | ✅ 已实现 | dim() 灰度思考内容 + 分隔线 + 状态追踪 |
| 4 | 统一 Usage 格式 | ✅ 已实现 | format_usage_line，含 verbose 预填充/解码细节 |
| 5 | REPL 增强 | ❌ 未开始 | 仍用 BufReader，无 rustyline |
| 6 | 详细度分级 | ⚠️ 部分 | 仅 verbose:bool，无 Verbosity 枚举
