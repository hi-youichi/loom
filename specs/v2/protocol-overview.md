# opencode 双协议架构概述

## 核心事实

opencode 服务器同时暴露两套 HTTP API 面：

- **v1 实例路由** — 无前缀，如 `/agent`、`/session/:id/prompt`，源码在 `packages/opencode/src/server/routes/instance/httpapi/groups/*.ts`
- **v2 协议路由** — `/api` 前缀，如 `/api/session/:id/prompt`，源码在 `packages/protocol/src/groups/*.ts`

两者在同一端口、同一 `HttpApiBuilder` 树下合并挂载（`server.ts:276-281`）。TUI 和 OpenChamber 使用同一份 SDK（`@opencode-ai/sdk/v2`），但连接拓扑不同。

## 两套消费者的差异

| | opencode TUI | OpenChamber |
|---|---|---|
| 连接 | 直连后端 | 浏览器 → Express 代理 → 后端 |
| 代理 | 无 | `pathRewrite: ^/api → ""` 剥离首个 `/api` |
| 认证 | Bearer Token | Basic Auth |
| 健康检查 | 无（靠 Promise.all 批量拉取） | `GET /global/health` → `{"healthy": true}` |
| SSE | `GET /global/event`（flat 信封） | `GET /api/event`（enriched 信封） |

代理的单层 `/api` 剥离导致 v2 SDK 内部已带 `/api` 的路径（如 `/api/session/:id`）到达后端时仍是 `/api/session/:id`，而 v1 裸路径（如 `/session/:id`）到达后端是 `/session/:id`。loom-server 必须双注册。

## v1 vs v2 的三层差异

**路径层** — 同一功能经常有两条路径：
- `POST /session/:id/abort`（v1）vs `POST /api/session/:id/interrupt`（v2）
- `GET /permission`（v1）vs `GET /api/permission/request`（v2）

**信封层** — v2 统一使用 `Location.response` 包装：
- v1：`GET /provider` → `[{...}, ...]`
- v2：`GET /api/provider` → `{location: {...}, data: [{...}, ...]}`

**Schema 层** — 同名字段语义不同：
- v1 agent 有 `name`；v2 agent 无 `name`（schema 故意省略，用 `id`）
- v1 session 列表返回裸数组；v2 返回 `Location.response` 信封

## 规模

opencode 定义约 **180 个端点**（v1 ~105 + v2 ~55，其中 ~40 个功能重叠）。loom-server 已实现约 100 个，未实现/TODO 约 45 个，stub/501 约 25 个。

详细端点矩阵见 `specs/v2/protocol-v1-v2-comparison.md`。
