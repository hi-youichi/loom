# OpenCode 协议文档索引

本目录只保留当前可用的协议基线、现状审计和 ACP 相邻文档。2025-08 的设计、端点矩阵、差异报告和重构方案已移至 `archive/2025-snapshots/`，不应被当作当前实现状态。

## 阅读顺序

1. 先读 [当前协议基线](CURRENT-CONTRACT.md)。
2. 再读 [现状审计](CURRENT-STATE-AUDIT.md)，了解尚未验证或仍不兼容的部分。
3. v2 session SSE 工作读 [v2 SSE 兼容规范](V2-SSE-COMPATIBILITY.md)。
4. 逐类型修正读 [v2 SSE 类型矩阵](LOOM-V2-SSE-TYPE-MATRIX.md)。
5. 按 PR 落地读 [v2 SSE 详细开发方案](LOOM-V2-SSE-DEVELOPMENT-PLAN.md)。
6. 查看 SSE 实际返回 JSON 时读 [SSE 返回结构参考](SSE-RETURN-STRUCTURES.md)。
7. 对接 SSE 客户端时按版本读 [v1 SSE API 规范](SSE-V1-API-SPEC.md) 或 [v2 SSE API 规范](SSE-V2-API-SPEC.md)。
8. Loom 具体改造读 [v2 SSE 修正方案](LOOM-V2-SSE-REMEDIATION.md)。
9. 实施 server SSE 自动化读 [server SSE 集成测试方案](SERVER-SSE-INTEGRATION-TEST-PLAN.md)。
10. 实施工作从 [当前任务](CURRENT-TASKS.md) 开始。
10. 需要追溯旧专题时，先读 [专题任务文档 Review](TASK-DOCUMENT-REVIEW.md)，再进入归档。
11. 做联调时以当前路由、handler、SSE 实现和实际探针结果为准。

## 目录

| 目录 | 内容 | 状态说明 |
| --- | --- | --- |
| `CURRENT-CONTRACT.md` | 当前代码可直接确认的 HTTP/SSE 基线 | 当前入口 |
| `CURRENT-STATE-AUDIT.md` | 已确认的差距、过时结论和验证缺口 | 当前入口 |
| `V2-SSE-COMPATIBILITY.md` | v2 session SSE schema、当前差距、映射与实施验收 | 当前入口 |
| `SSE-RETURN-STRUCTURES.md` | 三条 SSE stream、outer envelope 和全部 32 个事件的完整 wire data 结构 | 当前入口 |
| `SSE-API-SPEC.md` | SSE transport、端点、重连/replay、工具生命周期和客户端消费规则 | 当前入口 |
| `SSE-V1-API-SPEC.md` | legacy global SSE 的 wrapper、连接事件、part 消费与限制 | 当前入口 |
| `SSE-V2-API-SPEC.md` | v2 envelope、durable/live、session replay 与工具生命周期 | 当前入口 |
| `LOOM-V2-SSE-REMEDIATION.md` | Loom 双轨 v2 SSE 修正架构、文件改动和测试门禁 | 当前入口 |
| `LOOM-V2-SSE-TYPE-MATRIX.md` | 32 个 `session.next.*` 类型的字段、来源与修正动作 | 当前入口 |
| `LOOM-V2-SSE-DEVELOPMENT-PLAN.md` | 按 PR 拆分的 Rust 文件、接口、迁移、测试与回滚方案 | 当前入口 |
| `SERVER-SSE-INTEGRATION-TEST-PLAN.md` | Server SSE router/TCP/persistence/fixture/Provider 集成测试实施方案 | 当前入口 |
| `CURRENT-TASKS.md` | 当前可执行的协议任务、验收标准与优先级 | 当前入口 |
| `TASK-DOCUMENT-REVIEW.md` | 归档内每份专题文档的内容 review 和处置建议 | 当前入口 |
| `archive/2025-snapshots/` | 旧规格、审计、报告、设计、SSE 重构和 ACP 相邻资料 | 历史参考，非现状 |

## 权威性

发生冲突时按以下顺序判断：

1. 当前 `apps/server/src/routes.rs`、handler、`sse.rs` 与 `translator.rs`。
2. 当前测试和实际协议探针结果。
3. OpenCode 当前源码/SDK schema。
4. 本目录中的设计、审计与历史报告。

已确认的陈旧或相互矛盾结论见 [CURRENT-STATE-AUDIT.md](CURRENT-STATE-AUDIT.md)。
