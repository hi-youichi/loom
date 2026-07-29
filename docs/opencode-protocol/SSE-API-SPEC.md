# Loom Server SSE API 规范索引

状态：当前实现 wire contract。更新日期：2026-07-24。

SSE v1 和 v2 是不同的 wire contract，不可按同一 envelope 或重连模型处理：

| 版本 | 文档 | 端点 | 主要用途 |
| --- | --- | --- | --- |
| v1 | [v1 SSE API 规范](SSE-V1-API-SPEC.md) | `GET /global/event` | Legacy OpenCode TUI 全局事件 |
| v2 | [v2 SSE API 规范](SSE-V2-API-SPEC.md) | `GET /api/event`、`GET /api/session/:id/event` | v2 SDK/session durable event |

两者共用的 transport 规则：`GET`、`Content-Type: text/event-stream`、按空行而非 TCP chunk 切分 record、多行 `data:` 拼接，以及每 10 秒 SSE comment keepalive（`keepalive`）。comment 不得送入业务 reducer。JSON 编码失败的单个事件会被丢弃且连接保持打开。

完整 v2 `session.next.*` payload 字段见 [SSE 返回结构参考](SSE-RETURN-STRUCTURES.md)。实现权威来源为 `apps/server/src/sse.rs` 与 `apps/server/src/v2_event.rs`。
