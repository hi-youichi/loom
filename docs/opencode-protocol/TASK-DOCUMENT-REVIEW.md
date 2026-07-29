# 专题任务文档 Review

Review 日期：2026-07-24。范围是 `archive/2025-snapshots/` 的 38 份文档。结论以当前 `apps/server/src/{routes,sse,translator}.rs`、handler 和协议探针脚本为准；未运行真实 Provider/OpenChamber 的项目明确标为“需验证”。可执行替代项见 [CURRENT-TASKS.md](CURRENT-TASKS.md)。

处置标签：

- **保留归档**：可用作历史背景，不可作为当前任务依据。
- **重写后恢复**：主题仍有效，但任务、代码锚点或完成状态必须重建。
- **移除**：不属于 OpenCode 协议资料，或已没有保留价值。

## 总体结论

没有一份归档文档可以原样恢复为活跃任务文档。最接近可复用的是 ACP 官方索引和 OpenCode 事件 schema 附录，但二者都必须以当前上游源码重新取数。最严重的问题是将“路由已注册”“内部 StreamEvent 已重构”或“历史脚本曾通过”误写成“OpenCode v2 兼容已经完成”。

## Specs 与审计

| 文档 | Review | 处置 |
| --- | --- | --- |
| `specs/protocol-overview.md` | 双 API 面和 OpenChamber 单层 path rewrite 的模型仍有参考价值；端点总数、实现/桩数量未绑定当前提交。 | 重写后恢复 |
| `specs/protocol-v1-v2-comparison.md` | 端点清单及每项 Loom 状态是历史静态矩阵；不能代表当前 SDK 或当前 handler。 | 重写后恢复 |
| `specs/opencode-session-summary.md` | 把 OpenCode summary 与 compaction 的区分讲清楚，但调用点和 Loom 影响均为旧源码推断。 | 保留归档 |
| `specs/openchamber-loom-integration.md` | Provider/auth 设计包含已变更的假设与阶段状态；应以外部 host 合约和实际 OpenChamber 版本重做。 | 重写后恢复 |
| `audits/loom-vs-opencode-endpoints.md` | 2026-07-18 静态扫描，但之后的路由/translator 已变；“50 critical 缺失”等计数不可使用。 | 重写后恢复 |
| `audits/loom-vs-openchamber-endpoints.md` | 2026-07-24 的静态扫描仍混淆了 proxy 经过一次 path rewrite 后的 SDK/私有路径，并把部分当前路由判断为缺失。 | 重写后恢复 |
| `reports/protocol-code-diff-report.zh.md` | 2025-08 的 44% 一致性与 16 个 Critical 是历史审计结果。 | 保留归档 |
| `reports/protocol-fixes-applied.md` | 仅说明当时修了什么；测试数量、文件范围和“remaining issues”已过期。 | 保留归档 |

## 设计与 ACP 相邻任务

| 文档 | Review | 处置 |
| --- | --- | --- |
| `design/loom-server-v2-protocol-rollout.md` | “25/25 已完成、protocol gate 全绿”与当前脚本的 session delete 204 断言和 handler 的 200 JSON 相冲突。 | 重写后恢复 |
| `design/openchamber-loom.md` | 2025-08 的端点差距和运行时观察已过期；保留拓扑背景即可。 | 保留归档 |
| `design/openchamber-verify-method.md` | 具备验收清单价值，但启动、代理和返回值假设需按当前 OpenChamber 重写。 | 重写后恢复 |
| `design/opencode-subagent-compat.md` | 是未落地的子代理桥接方案，未有当前实现验证。 | 保留归档 |
| `design/thinking-to-thought.md` | reasoning 设计依赖旧事件字段与旧审计编号，不能直接实施。 | 重写后恢复 |
| `design/tool-calling-protocol.md` | 记录多次 2025-08 修补和环境观察；不是稳定的协议规范。 | 保留归档 |
| `acp-adjacent/acp-websocket-cli-ensure.md` | 明确是 2025-08 draft；CLI 参数和 server 生命周期须重新核对。 | 重写后恢复 |
| `acp-adjacent/acp-websocket-persistent-agent.md` | 2025-08 draft；其 `AcpHub`、连接全局状态和重连假设需和当前 ACP handler 重审。 | 重写后恢复 |
| `acp-adjacent/rust-agent-client-protocol-index.md` | ACP 官方资料部分更新到 2026-07-05；但附录 H 的 Loom/OpenCode 映射及其中的 rollout 引用是旧快照。 | 拆分后恢复：ACP 索引可保留，Loom 映射重写 |

## SSE Translator 专题

| 文档 | Review | 处置 |
| --- | --- | --- |
| `sse-translator/README.md` | A–G 的内部重构已发生，但“逐字段对齐 OpenCode v2”的结论不成立；当前输出仍以 `message.part.updated` 为主。 | 重写后恢复 |
| `01-opencode-architecture.md` | OpenCode processor 的历史架构参考，行号与当前源码未重新确认。 | 保留归档 |
| `02-part-lifecycle.md` | 生命周期推导对理解有帮助，但基于重构前 translator。 | 保留归档 |
| `03-tool-lifecycle.md` | 工具状态机设计有参考价值，具体 state/payload 需按当前代码复核。 | 保留归档 |
| `04-protocol-and-id.md` | v1 part 更新与 v2 delta 的分歧仍成立；具体 ID、事件和缺口状态已旧。 | 重写后恢复 |
| `05-error-handling.md` | OpenCode 重试/错误管线为历史研究，Loom 方案未验收。 | 保留归档 |
| `06-checklist.md` | 任务编号、文件清单和验证步骤都对应旧重构阶段。 | 重写后恢复 |
| `07-examples.md` | 是旧 wire payload 示例，不可用于当前抓包断言。 | 保留归档 |
| `08-stream-event-refactor.md` | 内部 block/delta 重构已成为代码事实，但设计中的具体迁移步骤已完成或失效。 | 保留归档 |
| `09-snapshot-patch.md` | Snapshot/Patch 为未验证的设计提案，不能视为现有能力。 | 保留归档 |
| `10-compaction.md` | Compaction 是未落地/未验证的设计，依赖旧的 event/route 假设。 | 重写后恢复 |
| `11-protocol-impact.md` | token 和事件影响表已落后当前 Usage 结构。 | 重写后恢复 |
| `12-integration-test-plan.md` | 计划中的 `stream_integration.rs` 当前不存在；任务未完成，测试方案需按现有 APIs 重建。 | 重写后恢复 |
| `13-field-gap-plan.md` | G1 已被当前 Usage 改名解决；G2–G5 仍需验证，原计划不能原样执行。 | 重写后恢复 |
| `16-runtime-validation.md` | 针对旧 translator 的对抗性分析；近期 StreamEvent/translator 改动后结论需重跑。 | 重写后恢复 |
| `appendix-a-opencode-v2-schema.md` | 上游 schema 摘录有价值，但没有固定 OpenCode commit，字段/事件可能漂移。 | 重写后恢复 |
| `appendix-b-sse-payload-examples.md` | 人工构造的旧样例，不是当前 server 的抓包。 | 保留归档 |
| `appendix-c-openchamber-integration.md` | token 字段仍写旧结构，且没有对当前前端版本验收。 | 重写后恢复 |
| `appendix-d-opencode-events-full.md` | 事件目录适合作为研究线索，但状态表与当前 Loom 代码不一致。 | 重写后恢复 |
| `TODO.md` | G1 与完成测试数过时；H/I 是否完成没有当前证据。 | 重写后恢复 |
| `workflow-failure-analysis.md` | 是一次 2025-08 workflow 运行事故，不属于 OpenCode 协议或当前任务设计。 | 移除 |

## 重新启动专题工作的顺序

1. 从当前 OpenCode SDK/schema 生成不可手改的端点和事件清单，并记录版本/commit。
2. 以当前 Loom handler 运行探针，区分“路由存在”“空/stub”“语义兼容”。
3. 先确定兼容目标：OpenChamber 当前需求，还是完整 OpenCode v2 `session.next.*`。
4. 仅在真实 SSE 抓包和 OpenChamber 验收通过后，将对应设计恢复为活跃任务文档。
