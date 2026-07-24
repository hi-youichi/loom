# OpenCode 协议当前任务

更新日期：2026-07-24。本文替代归档中的 rollout、endpoint audit、SSE translator TODO 和 OpenChamber 验收计划。任务必须以当前 OpenCode SDK/schema 的固定版本为输入；没有版本或真实探针证据时，不能标记完成。

## P0：建立可信兼容性基线

### P0-1：统一 session delete 契约

当前 `apps/server/src/handlers/session.rs::delete_session` 返回 `200 {"success":true}`，而 `scripts/check-protocol.ps1` 与 `.sh` 仍断言 204。

- [ ] 从目标 OpenCode SDK/schema 确认 v1 和 v2 delete 的真实 status/body。
- [ ] 让 handler、`apps/server/tests/{protocol,endpoint_integration}.rs` 与两份协议脚本使用同一契约。
- [ ] 运行 `scripts/check-protocol.ps1` 和 `.sh`，记录结果。

验收：脚本和测试不再因 delete 的状态码或 JSON body 分歧而出现假失败/假通过。

### P0-2：重新生成端点矩阵

归档中的矩阵和审计没有固定当前 SDK 版本，且路由在 2026-07 已变更。

- [ ] 记录 OpenCode checkout 的 commit 与 SDK/schema 路径。
- [ ] 从 SDK 生成 v1/v2 方法和路径清单。
- [ ] 从 `apps/server/src/routes.rs` 提取 Loom 路由。
- [ ] 对每个路径标注：`semantic`、`stub`、`missing` 或 `loom-only`；不能只标“已注册”。

验收：矩阵可重复生成，并替代 `archive/2025-snapshots/{specs,audits}/` 中的状态计数。

### P0-3：SSE 实测与自动断言

当前 `sse.rs` 已提供全局/会话流、连接事件、10 秒业务 heartbeat 与 `after` replay；仍缺少 **Loom-only MockLlm** 驱动的 wire-level 断言与 100% 返回结构 coverage gate。

- [ ] 用 MockLlm/MultiRoundMockLlm 覆盖单回合、多回合、工具调用、失败与取消的 SSE，不依赖外部 Provider。
- [ ] 为 v1、v2 与 session scoped stream 的所有返回结构断言 envelope、字段、`sessionID` 过滤、`after` replay、heartbeat 和断线后的重新订阅。
- [ ] 把测试放到 `apps/server/tests/`，生成并强制 `sse-structure-coverage.json == 100%`，不依赖人工抓包作为唯一证据。

验收：每次 CI 验证实际 event name、payload 的所有结构，且返回结构 coverage 为 100%。

## P1：明确兼容目标并关闭主路径缺口

### P1-1：确定 SSE 兼容层级

v2 durable session SSE 的首个切片已开始实现：prompt、step、text 和 reasoning 的边界会双发 `session.next.*`；tool、live delta 和其余能力事件尚未实现。

- [x] 决定以完整 OpenCode v2 为目标；采用 legacy/v2 双轨 event bus。
- [~] 实现 `session.next.step/text/reasoning/tool.*` 的映射与 replay 策略；当前仅完成 prompt/step/text/reasoning durable 边界，尚未持久化到重启后。
- [ ] 若选择 OpenChamber：记录 v1-part 兼容边界，并删除“逐字段 v2 对齐”的表述。

验收：`CURRENT-CONTRACT.md`、OpenChamber adapter 与测试对同一事件模型达成一致。

### P1-2：实现或明确拒绝空/stub 功能

`GET /api/skill` 已注册但返回空列表；其他兼容路由同样可能仅返回空 envelope 或固定成功值。

- [ ] 从新端点矩阵列出被目标客户端实际调用的 stub。
- [ ] 对每项选择实现、显式 `501`，或从客户端兼容范围排除。
- [ ] 首先处理 skill registry、session todo/diff、project/current 和 OpenChamber 主流程触及的项目。

验收：没有以 2xx 空响应伪装“已支持”的主路径功能。

### P1-3：OpenChamber 外部后端验收

- [ ] 按 `AGENTS.md` 的 external-host 模式启动 Loom 与 OpenChamber。
- [ ] 验证 `/global/health`、认证、Provider/Model、会话创建、prompt、SSE、取消和错误展示。
- [ ] 将真实调用路径和失败响应写入新端点矩阵。

验收：浏览器端主流程通过，且浏览器网络记录与矩阵一致。

## P2：后续能力

- [ ] Session summary/compaction：先确认目标 OpenCode 语义，再决定是否实现。
- [ ] Snapshot/patch/revert：作为独立功能设计，不把归档方案当作现有实现。
- [ ] 子代理 bridge：重新验证 OpenChamber 是否实际消费 child session 语义后再启动。
- [ ] ACP WebSocket 持久 agent：与 HTTP/SSE 兼容工作分开立项；归档中的 draft 不能直接实施。

## 完成规则

一项任务只有同时满足以下条件才能标记完成：

1. 当前代码有实现或明确的支持边界；
2. 自动测试或协议探针覆盖；
3. 对外 HTTP/SSE 形状经真实客户端或固定 schema 验证；
4. `CURRENT-CONTRACT.md` 和新矩阵已同步。
