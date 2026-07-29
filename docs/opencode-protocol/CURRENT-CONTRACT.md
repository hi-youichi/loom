# OpenCode 协议当前基线

最后核对：2026-07-24。本文只记录可从当前工作树直接确认的行为；不把路由存在等同于完整功能兼容。

## HTTP 路由

- 服务同时注册 OpenCode v1 风格的裸路径和 v2 `/api/*` 路径；以 `apps/server/src/routes.rs` 为唯一的路由事实来源。
- `GET /global/health` 与 `GET /api/health` 均已注册。OpenChamber 外部后端模式仍以 `/global/health` 的 `healthy: true` 为启动门槛。
- `GET /api/skill` 已注册，但当前 handler 返回一个空的 `Location.response` 列表；不得宣称已有 skill registry。
- `DELETE /session/:id` 的当前实现返回 `200 {"success":true}`。任何协议探针、测试或客户端契约都必须与这一实现或经重新确认的 OpenCode 契约保持一致。

## SSE

- `GET /global/event`、`GET /api/event` 和 `GET /api/session/:id/event` 已注册。
- legacy `/global/event` 仍发送 `server.connected`、每 10 秒的业务 `server.heartbeat` 和 keepalive 注释；v2 session stream 只发送 durable `session.next.*`，保留注释 keepalive。
- `/api/session/:id/event` 使用 per-session numeric `after` cursor；生产 durable log 位于 `$LOOM_HOME/server/v2-events/`，启动会恢复 sequence。
- legacy part 流仍保留，同时 v2 已发布 prompt/step/text/reasoning/tool、agent/model/moved/shell、compaction/context/revert 的已实现事件；retry、synthetic 与没有上游 chunk 的 tool input delta 仍没有真实来源。

## 验证规则

1. 先运行当前单元/集成测试，再运行 `scripts/check-protocol.ps1` 或 `.sh`。
2. 在宣称兼容前，必须检查脚本中所有期望状态码和响应形状是否与 handler 一致。
3. 使用真实 Provider 抓取 SSE，并断言事件名、payload、重放 cursor、session 过滤和断线重连。
