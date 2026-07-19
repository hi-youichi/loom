# ACP WebSocket 后续工作

## 当前范围

`loom-server` 现已提供 `GET /acp` WebSocket 入口。每个文本帧承载一条 ACP JSON-RPC 消息，并复用 `apps/acp` 的协议 handler。该入口当前适合单一、持续连接的 CLI。

## P0：连接隔离（阻断多客户端）

- [x] 删除 `apps/acp/src/tools/client_bridge.rs` 中的 `GLOBAL_BRIDGE` / `OnceLock`。
- [x] 将 `ClientBridge` 作为 `AcpConnection` 的依赖注入到文件与终端工具；按 ACP session 找到所属连接，不能使用进程级全局状态。
- [ ] 为同一进程中两条 ACP WebSocket 同时初始化、同时发起 `fs/read_text_file` / terminal 请求添加集成测试，验证请求不会串到另一个客户端。

验收：第二条客户端连接、断开或初始化不能改变第一条客户端的 client capabilities、权限请求目标或终端目标。

## P1：服务端会话中心（AcpHub）

- [x] 在 `apps/server/src/state.rs` 增加 `AcpHub`，由 `AppState` 持有。
- [ ] 将 ACP session 的 thread ID、cwd、MCP 配置、模型/模式、当前 run cancellation 与 owner 身份移入 `AcpHub`；不要让它们随 WebSocket 生命周期销毁。
- [ ] 把 `LoomAcpAgent` 拆为无连接的 session/run core 与连接特有的 capabilities/bridge/output sink。
- [ ] 每个 ACP session 使用 actor 或串行命令队列，拒绝同一 session 的并发 prompt，或返回清晰的 JSON-RPC 错误。

验收：同一 CLI 断开并重新连接后，可 `session/load` 已有 session；后续 prompt 继续使用原 thread ID 和配置。

## P1：断线、重连与事件恢复

- [ ] 在 `AcpHub` 中维护每 session 的递增 event cursor 和有界 `session/update` 缓冲区（容量、TTL 均可配置）。
- [ ] 定义 Loom 扩展字段 `_meta.eventCursor` / `_meta.resumeFrom`，在重新附着时重放遗漏通知；标准 ACP 客户端不识别扩展时至少发送当前 session 状态。
- [x] 默认断线策略设为 `persist`：连接关闭不自动取消 run；保留显式 `session/cancel`。
- [ ] 支持可配置的 `disconnect_policy=cancel`，供短命令/CI 使用。
- [ ] 对没有重连的 session/run 设置 TTL 和后台清理，避免永久占用 MCP、PTY 或内存。

验收：运行中的 prompt 在 WS 被中断后继续执行；客户端重连后能收到缺失的更新或明确的当前最终状态。

## P1：权限与反向 RPC

- [ ] 将 `session/request_permission` 记录为 `PendingPermission`，绑定 session、run、connection owner 与过期时间。
- [ ] 客户端离线时暂停权限相关工具调用；绝不因断线自动批准。
- [ ] 默认权限 TTL 到期后拒绝该工具调用，并向 run 和重连客户端发送可诊断的更新。
- [ ] 对 fs/terminal 等 client-side RPC 加入连接版本检查：重连后只能向当前已绑定连接发起请求。

验收：工具在权限等待期间断线不会执行；重连可继续回复；TTL 到期行为可预测且测试覆盖。

## P2：网络安全与运营

- [x] 为 `/acp` 单独校验 `Origin`；CORS middleware 不构成 WebSocket Origin 防护。默认仅允许 loopback browser origin，远程来源通过 `LOOM_ACP_ALLOWED_ORIGINS` 明确配置。
- [ ] 复用 HTTP Bearer 鉴权并把认证主体写入 `AcpConnection` / session owner；拒绝跨主体接管会话。
- [ ] 增加最大帧大小、初始化超时、空闲 ping/pong、每主体连接数和并发 run 限制。
- [ ] 增加 ACP 连接/重连/run/权限/事件丢弃的结构化指标与审计日志（不要记录 prompt 内容或 token）。

验收：未认证连接、非法 Origin、超大帧和跨主体 session/load 均被拒绝；正常本地开发仍可在未配置 token 时运行。

## P2：测试与文档

- [ ] 新增 WS ACP 集成 harness：initialize → session/new → prompt → `session/update` → cancel。
- [ ] 覆盖 Ping/Pong、binary frame 拒绝、无效 JSON-RPC、客户端异常关闭、重连和权限超时。
- [x] 在 [ACP WebSocket 接入文档](acp-websocket.md) 中记录 `/acp` URL、认证、CLI 重连行为和 stdio 兼容入口。
- [ ] 在协议升级测试中按协商的 `protocolVersion` 与 capabilities 断言行为，不依据 Rust crate 版本判断 wire 兼容性。
