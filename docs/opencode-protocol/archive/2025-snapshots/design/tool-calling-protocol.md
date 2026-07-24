| 2025-08-19 | 1.8 | 当前状态: PR-pre、PR-0、PR-1+2+3、PR-5、P1(两项) 全部完成。剩余项:
- **PR-4**: act_executor 实现 ToolOutput + streaming tool (`act_executor.rs` + Tool trait streaming,~200 LOC)
- **P2**: Anthropic 适配层 (`client/anthropic/` 新建,~500 LOC)
- **P3**: ToolSpec 超长 description → skill 引用 (`tool.rs`,~100 LOC)
- **剩余 3 项预计总 LOC 约 800 行,风险中等。核心 tool 信息可见化已完成(P0),现在 TUI 已经能看到真实的 `call_id`/`input`/`output`/`title`。 |
| 2025-08-19 | 1.9 | **OpenChamber 集成发现** (重要集成提示):用户用 OpenChamber (Bun,3000) → loom-server (Rust/Axum) 链路时,OpenChamber 的 `/api/*` proxy 持续返回 503/504 (body `{"error":"OpenCode service unavailable"}`)。**根因不在 loom-server**,也不在网络配置,而是 `openchamber-feat-dev/packages/web/server/lib/opencode/network-runtime.js:87` 在 `state.openCodePort` 为 null 时 throw,forwardSseRequest catch 后返回 503。OpenChamber 的 `state.openCodePort` 由 `lifecycle.js` 三分支设置:
1. `isOpenCodeProcessHealthy()` 复用现有 (要求 /global/health 存在)
2. **`ENV_SKIP_OPENCODE_START=true && ENV_EFFECTIVE_PORT`** — skip-start mode,直接 trust env port (lifecycle.js:806-816)
3. `ENV_EFFECTIVE_PORT && probeExternalOpenCode(...)` — auto-detect via /global/health probe

**正确启动链路**(OpenChamber **源码零修改**):
```cmd
set OPENCODE_PORT=3902
set OPENCHAMBER_SKIP_OPENCODE_START=true
cd openchamber-feat-dev
node packages\web\server\index.js --port 3000
```

loom-server 单跑 `cargo run -p loom-server -- --port 3902` 即可。OpenChamber 输出 `Using external OpenCode server at http://localhost:3902 (skip-start mode)` + `Detected OpenCode port: 3902` 即表示集成成功。**注意**:`OPENCODE_PORT` / `OPENCHAMBER_OPENCODE_PORT` / `OPENCHAMBER_INTERNAL_PORT` 三个 env var 任一有效即可,`OPENCHAMBER_OPENCODE_HOST=http://localhost:4096` 形态也支持(详见 `env-config.js`)。**剩余 503** 通常只剩 /api/event (SSE) — 等待 SSE 上游响应,超时即 504,正常现象。
| 2025-08-19 | 1.10 | **Deep-merge bug 修复**:`emit_tool_part` 的浅 merge 用 `existing.insert("time", delta["time"])` 覆盖了整个 `time` 对象,导致 `ToolEnd` 后 `state.time.start` 丢失。改为递归 `deep_merge` (对象按 key 递归,其他类型替换)。新增回归测试 `tool_end_preserves_time_start_from_tool_call`。CDT 实测端到端:`ToolCall { time: {start: ts, end: 0} }` → `ToolEnd { time: {end: ts'} }` 后 part 仍保留 start,且 start < end |
| 2025-08-19 | 1.11 | **磁盘持久化修复**:实现 `JsonFileStore`(`apps/server/src/storage.rs:108-308`)后,**关键发现**:`main.rs:71` 调的是 `new_server_state()`(无 `store`),而非 `new_server_state_with_store()`。改为后者后, sessions/messages/parts 真持久化到 `~/.loom/storage/{sessions,messages,parts}/*.json`。**Load-on-startup** 重新激活 — session 重启不丢失。<br>**根因链**:① `state.rs:InMemoryStore` 默认无 disk 写入 ② `main.rs` 从未传 `store=Some(...)` ③ `JsonFileStore` 实现被引但从未被构造 ④ 旧 `session-*.json` 文件来自其他系统,ID 格式 `sess_*` 与之不兼容<br>**修复**:`main.rs:7,71` 改 `new_server_state_with_store()`,`new_server_state_with_store()` 默认实例化 `JsonFileStore`。新 session 创建 → 写 `sess_<uuid>.json`,重启 → 从盘加载。**用户报错 session `sess_7f981b...` 仍 404** 是因为它根本没在我们 loom-server 创建过(在 OpenChamber HMR cache 或其他子进程),新 session 已 work。 |
