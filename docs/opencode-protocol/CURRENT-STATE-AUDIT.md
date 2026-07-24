# OpenCode 协议文档现状审计

审计日期：2026-07-24。范围是本目录的文档与当前工作树代码；没有把未运行的端到端流程标记为已验证。

## 结论

目录中大部分协议设计、审计和状态表写于 2025-08。它们适合解释目标和历史差距，但不能直接作为当前兼容性结论。当前代码已经发生实质变化，同时仍存在一个可复现的协议检查脚本与 handler 响应不一致问题。

## 已过时或不正确的内容

| 文档/结论 | 判定 | 当前证据与应采取的读法 |
| --- | --- | --- |
| `archive/2025-snapshots/specs/protocol-overview.md` 的“Loom 已实现约 100、未实现/TODO 约 45、stub 约 25” | 已归档 | 这是未注明提交基线的历史计数。当前路由已含 `/api/skill`、session SSE replay 等后来加入的路径，不能用该数字排期。 |
| `archive/2025-snapshots/reports/protocol-code-diff-report.zh.md` 的总体“44% 一致性”、缺失 `session.next.*` 和各 Critical 列表 | 已归档 | 报告日期为 2025-08-19；严重度和数量不能当作现状。 |
| `archive/2025-snapshots/reports/protocol-fixes-applied.md` 的“18 项修复已应用、8 个集成测试仍失败” | 已归档 | 当前测试与实现已变化，应运行当前测试而非引用历史数量。 |
| `archive/2025-snapshots/sse-translator/README.md` 的“目标是 OpenCode v2 `session.next.*`”与“A–G 已完成，因此已对齐” | 已归档 | 当前已新增 prompt/step/text/reasoning 的 v2 durable 双发，但 Tool、live delta、其余事件和重启持久化仍未实现，不能宣称完整 taxonomy 对齐。 |
| `archive/2025-snapshots/sse-translator/TODO.md` 的 G1 token 结构 | 已归档 | 当前提交 `017abeba` 已改为 `input/output/reasoning/cache_read/cache_write`，且 translator 输出对应的嵌套 `cache` 结构。 |
| `archive/2025-snapshots/audits/loom-vs-opencode-endpoints.md` 中 `/api/skill` “缺失端点”的表述 | 已归档 | 当前路由已注册 `GET /api/skill`，但 handler 仍返回空列表；功能缺口仍在。 |
| `archive/2025-snapshots/design/loom-server-v2-protocol-rollout.md` 将 P0–P3 多项标为完成 | 已归档 | `scripts/check-protocol.*` 对删除 session 仍期望 204，而 handler 返回 `200 {"success":true}`；不能再引用“protocol gate 全绿”。 |

## 仍然有效、但尚未验收的方向

- v1 与 v2 双路由、OpenChamber 单层 `/api` path rewrite，以及 `/global/health` 契约仍是关键约束。
- session scoped SSE、`server.connected` 与业务 `server.heartbeat` 已有当前代码实现；是否完全符合 OpenCode 的 replay、cursor 和事件 payload 契约尚未做端到端断言。
- `/api/skill`、integration、部分 MCP/PTY/experimental 路由应区分“可路由的空/stub 响应”和“真实功能实现”。
- OpenChamber 端到端验收仍需要真实 Provider 与前端运行；历史文档不能代替该验收。

## 必须先修正的验证基线

1. 统一 `DELETE /session/:id` 的预期：当前实现是 200 JSON，而两份 `scripts/check-protocol.*` 期望 204。
2. 增加独立的 SSE 契约测试：断言实际事件名、v1/v2 envelope、`after` replay 和 session 过滤，而不是只检查连接能打开。
3. 明确兼容目标：若目标是 OpenCode v2，则补齐或明确拒绝 `session.next.*` 事件族；若目标仅是 OpenChamber 当前渲染路径，则更新文档，不再宣称“逐字段对齐 OpenCode v2”。
4. 从当前 OpenCode SDK 重新生成端点、schema 与状态矩阵，并为每项记录源码提交或版本。
