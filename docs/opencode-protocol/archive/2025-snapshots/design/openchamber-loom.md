# openchamber ↔ loom-server 对接设计文档

> 日期: 2025-08-19
> 状态: 分析完成，待实现

## 任务清单

### Phase 1 — Provider/Model 选择可用 (P0)
- [ ] 改动 A: 启动 loom-server (`cargo run -p loom-server -- serve --host 127.0.0.1 --port 18081`)
- [ ] 改动 B: 设置 openchamber 环境变量 `OPENCODE_HOST` + `OPENCODE_SKIP_START`，启动 openchamber
- [ ] 改动 C: 重写 `bootstrap.rs` 中 `get_provider_list()` + `get_config_providers()` 从 config.toml 读取
- [ ] 改动 D: `routes.rs` 注册 `/model` 无前缀路由别名
- [ ] 验证: `cargo build -p loom-server` 编译通过
- [ ] 验证: 直接 curl 三个端点返回非空数据 (`/provider`, `/config/providers`, `/model`)
- [ ] 验证: openchamber 联调 — Provider 列表 + 模型选择

### Phase 2 — FS 完整化 (P0)
- [ ] 新增 `/api/fs/{mkdir,stat,read,write,delete,rename,reveal,raw}` 路由
- [ ] 新增 `/api/config/settings` GET/PUT + `/api/config/reload` POST

### Phase 3 — Git 基础 (P0)
- [ ] 新增 `handlers/git.rs`，用 `std::process::Command` 调 git CLI
- [ ] 实现 check/status/diff/stage/unstage/commit/log/branches
- [ ] 注册到 `/api/git/*` 路由

### Phase 4 — Terminal (P1)
- [ ] 使用 `portable-pty` 或 `tokio::process` 创建 PTY
- [ ] 实现 create/input/resize/close/stream 端点 + WebSocket

### Phase 5 — Git 进阶 (P1)
- [ ] push/pull/fetch, worktrees, rebase/merge/stash, identities

### Phase 6 — 可选模块 (P2)
- [ ] GitHub (`/api/github/*`)
- [ ] Push (`/api/push/*`)
- [ ] Client Auth (`/api/client-auth/*`)
- [ ] Small Model (`/api/small-model/*`)
- [ ] Dictation (`/api/dictation/*`)

---

## 1. 背景

**openchamber-feat-dev** 是 opencode 的 fork（bun monorepo，含 web/electron/mobile/vscode），前端通过 `runtimeFetch('/api/...')` 和 `@opencode-ai/sdk` 调用后端。

**loom-server** 是 Rust/Axum HTTP+SSE 服务器（`apps/server/`），实现 Loom agent 内核，已完成 opencode v1+v2 协议路由注册。

**目标**: 让 loom-server 作为 openchamber 的后端，替代 opencode。

---

## 2. 架构：openchamber 如何连接后端

### 2.1 默认行为（三进程孤立）

```
┌─────────────────────┐     spawn      ┌─────────────────────┐
│  openchamber web     │ ──────────────►│  opencode (npm)     │
│  Express + Vite      │                │  opencode serve     │
│  port: 3000          │ ◄──────────── │  port: 4096         │
└─────────────────────┘   /api/* 代理   └─────────────────────┘
                                               ▲ 完全无关 ▼
                                      ┌─────────────────────┐
                                      │  loom-server (Rust)  │
                                      │  port: 18081         │
                                      └─────────────────────┘
```

openchamber 启动时自动 spawn `opencode serve` 子进程，然后代理所有 `/api/*` 请求给它。loom-server 完全未被使用。

### 2.2 切换到 loom-server（替代 opencode）

**Step 1 — 启动 loom-server（端口 18081）**

```powershell
cd C:\Users\heycj\dev\worktrees\loom\cli-server-backend
cargo run -p loom-server -- serve --host 127.0.0.1 --port 18081
```

**Step 2 — 启动 openchamber（指向 loom-server）**

```powershell
$env:OPENCODE_HOST = "http://127.0.0.1:18081"
$env:OPENCODE_SKIP_START = "true"
cd C:\Users\heycj\dev\openchamber-feat-dev
bun run packages/web/dev
```

设置 `OPENCODE_HOST` 后，openchamber 进入外部模式（`env-config.js:18`，`lifecycle.js:802`）：
- 不 spawn opencode（跳过 `opencode serve` 子进程）
- proxy 目标变为 `OPENCODE_HOST`（即 loom-server:18081）
- `state.isExternalOpenCode = true`
- `state.openCodeBaseUrl = origin`

| 环境变量 | 作用 | 示例值 |
|---------|------|--------|
| `OPENCODE_HOST` | 外部后端 URL（含端口），设置后不 spawn opencode | `http://127.0.0.1:18081` |
| `OPENCODE_SKIP_START` | `"true"` 跳过启动（需配合 `OPENCODE_HOST`） | `true` |

替换前后的端口对应：

| 角色 | 替换前 | 替换后 |
|------|--------|--------|
| 前端 | openchamber:3000 | openchamber:3000（不变） |
| 后端 | opencode:4096（自动 spawn） | **loom-server:18081**（手动启动） |

### 2.3 健康检查

openchamber 启动时调用 `GET /global/health` 验证后端（`network-runtime.js:49`）。

loom-server 已满足：`GET /global/health` → `{ "healthy": true, "kind": "external-kernel", "version": "0.4.0" }` ✅

### 2.4 认证

openchamber 通过 `OPENCODE_SERVER_PASSWORD` 生成 Basic Auth header（`auth-state-runtime.js:47`）。loom-server 默认不启用认证，两端兼容。

---

## 3. URL 路由机制（关键发现）

### 3.1 代理 pathRewrite

openchamber 代理层（`proxy.js:703`）的 `pathRewrite: { '^/api': '' }` 只去掉**第一个** `/api` 前缀。

### 3.2 SDK baseUrl

SDK client baseUrl = `/api`（`client.ts:41`）。SDK 方法的 `url` 属性拼接在 baseUrl 后。

### 3.3 URL 构造链路

| SDK 方法 | SDK url | 浏览器发送 | proxy 重写后 | 后端收到 |
|---------|---------|-----------|-------------|---------|
| `Provider.list()` | `/provider` | `/api/provider` | `/provider` | `GET /provider` |
| `Provider.auth()` | `/provider/auth` | `/api/provider/auth` | `/provider/auth` | `GET /provider/auth` |
| `Model.list()` | `/api/model` | `/api/api/model` | `/api/model` | `GET /api/model` |
| `Provider2.list()` | `/api/provider` | `/api/api/provider` | `/api/provider` | `GET /api/provider` |
| `Session.list()` | `/session` | `/api/session` | `/session` | `GET /session` |
| `runtimeFetch` | — | `/api/config/providers` | `/config/providers` | `GET /config/providers` |

后端收到混合路径：部分无前缀（`/provider`），部分有前缀（`/api/model`）。loom-server **已注册双路径**（`/provider` + `/api/provider`，`/config` + `/api/config`），大部分路径匹配没问题。

### 3.4 openchamber 自有路由

openchamber 中间层（`lib/opencode/routes.js`）拦截部分 API 请求自己处理，**不代理给后端**：

| 端点 | 说明 |
|------|------|
| `GET/PUT /api/config/settings` | openchamber 自有设置 |
| `GET /api/config/opencode-resolution` | opencode CLI 路径解析 |
| `GET /api/opencode/health` | opencode 进程健康 |
| `GET /api/opencode/version` | opencode 版本 |
| `GET /api/provider/:id/source` | provider 来源信息 |
| `DELETE /api/provider/:id/auth` | 撤销 provider 认证 |
| `GET /api/config/themes` | 主题列表 |
| `POST /api/config/reload` | 重载配置 |

这些请求不会到达 loom-server。

---

## 4. 运行时调试发现（2025-08-19 实测）

通过 Chrome DevTools 对运行中的 openchamber (localhost:3000) 调试，确认模型选择为空的根因：

### 4.1 Provider 列表为空

| 端点 (openchamber:3000) | 状态 | 响应 |
|------------------------|------|------|
| `GET /api/provider` | 200 | `{all:[], connected:[], default:{}}` ← 空 |
| `GET /api/config/providers` | 200 | `{default:{}, providers:[]}` ← 空 |
| `GET /api/model` | 200 | `{location, data:[]}` ← 空 |

**根因**：loom-server 的 `get_provider_list()` 和 `get_config_providers()` 返回硬编码空列表（`bootstrap.rs:270-277`），不从 `~/.loom/config.toml` 读取实际数据。

对比 opencode:4096：`GET /provider` 返回 150+ providers，`GET /model` 返回 20+ models。

### 4.2 无法添加 Provider

点击 "Add new provider" 后，前端调用 `opencodeClient.getSdkClient().provider.auth()`（`ProvidersPage.tsx:192`），映射到 `GET /provider/auth`，返回 404。

**根因**：loom-server 没有 `/provider/auth` 路由。

---

## 5. 端点差距矩阵

### 5.1 已就绪 ✅

| API 组 | 后端路由 | 备注 |
|--------|---------|------|
| Session/Prompt | `routes.rs:228-345` | CRUD + prompt + command + shell + interrupt |
| Messages | `routes.rs:532-582` | 含 part 级操作 |
| SSE 事件 | `routes.rs:783-785` | 全局 + session 级 |
| Bootstrap | `routes.rs:113-172` | config + provider + agent + model + skill |
| Permission | `routes.rs:358-396` | 含 reply |
| Question | `routes.rs:398-424` | 含 reply + reject |
| VCS | `routes.rs:199-210` | 含 diff raw |
| Health | `routes.rs:212-213` | `/api/health` + `/global/health` |
| File | `routes.rs:456-493` | read + content + find |

### 5.2 需修复 🔴

| API 组 | 问题 | 修复 |
|--------|------|------|
| `GET /provider` | 返回硬编码空列表 | 从 config.toml 读取 |
| `GET /config/providers` | 返回硬编码空列表 | 从 config.toml 读取 |
| `GET /model` (无前缀) | 缺失 | 新增路由别名 |

> 注：`/provider/auth` 是 openchamber 扩展端点，opencode 本身也没有。loom-server 对标 opencode，不实现此端点。

### 5.3 完全缺失 ❌

**Git API** (`/api/git/*`) — 约 55 个端点：check, status, diff, stage, unstage, commit, push, pull, fetch, branches, checkout, log, worktrees, stashes, rebase, merge, cherry-pick, identities 等。前端 `gitApiHttp.ts` 调用。所有端点通过 `x-opencode-directory` header 传递工作目录。

**Terminal API** (`/api/terminal/*`) — 8 个端点：create, input, resize, close, restart, force-kill, stream (SSE), ws (WebSocket)。

**Small Model** (`/api/small-model/*`) — commit message 生成、PR description、笔记摘要、TTS 摘要。

**其他可选** — GitHub (`/api/github/*`), Push (`/api/push/*`), Client Auth (`/api/client-auth/*`), Dictation (`/api/dictation/*`), OpenChamber 扩展 (`/api/openchamber/*`)。

---

## 6. 代码修改方案

### 改动 A — 启动 loom-server（端口 18081）

```powershell
cd C:\Users\heycj\dev\worktrees\loom\cli-server-backend
cargo run -p loom-server -- serve --host 127.0.0.1 --port 18081
```

### 改动 B — openchamber 启动配置（0 代码改动）

```powershell
$env:OPENCODE_HOST = "http://127.0.0.1:18081"
$env:OPENCODE_SKIP_START = "true"
cd C:\Users\heycj\dev\openchamber-feat-dev
bun run packages/web/dev
```

### 改动 C — 修复 v1 provider 返回空数据 🔴

**文件**: `apps/server/src/handlers/bootstrap.rs:270-277`

将硬编码空返回改为从 `~/.loom/config.toml` 读取：

```rust
pub async fn get_config_providers() -> Json<Value> {
    let cfg = match config::load_full_config(CONFIG_APP_NAME) {
        Ok(c) => c,
        Err(_) => return Json(json!({"providers": [], "default": {}})),
    };
    let providers: Vec<Value> = cfg.providers.iter().map(|p| {
        json!({
            "id": p.name,
            "name": p.name,
            "models": p.models.iter().map(|m| &m.id).collect::<Vec<_>>(),
            "default_model": p.model,
        })
    }).collect();
    let default = cfg.providers.first()
        .and_then(|p| p.model.as_ref())
        .map(|m| json!({m: {}}))
        .unwrap_or(json!({}));
    Json(json!({"providers": providers, "default": default}))
}

pub async fn get_provider_list() -> Json<Value> {
    let cfg = match config::load_full_config(CONFIG_APP_NAME) {
        Ok(c) => c,
        Err(_) => return Json(json!({"all": [], "default": {}, "connected": []})),
    };
    let all: Vec<Value> = cfg.providers.iter().map(|p| {
        json!({
            "id": p.name,
            "name": p.name,
            "models": p.models.iter().map(|m| &m.id).collect::<Vec<_>>(),
        })
    }).collect();
    let connected: Vec<String> = cfg.providers.iter()
        .map(|p| p.name.clone())
        .collect();
    let default = cfg.providers.first()
        .map(|p| json!({"providerID": p.name, "modelID": p.model}))
        .unwrap_or(json!({}));
    Json(json!({"all": all, "default": default, "connected": connected}))
}
```

### 改动 D — 路由注册 🟡

**文件**: `apps/server/src/routes.rs`，在 `build_router()` 中添加：

```rust
// ─── openchamber compat: model 无前缀路由 ───
.route("/model", get(handlers::bootstrap::get_api_models))
```

---

## 7. 修改清单

### loom-server（Rust）

| # | 文件 | 改动 | 优先级 |
|---|------|------|--------|
| B | openchamber 启动 | 仅环境变量，0 代码改动 |
| C | `handlers/bootstrap.rs` | 重写 `get_provider_list()` + `get_config_providers()` | 🔴 P0 |
| D | `routes.rs` | 注册 `/model` 无前缀路由 | 🟡 P1 |

### openchamber（JS）

| # | 文件 | 改动 |
|---|------|------|
| A | loom-server 启动 | `cargo run -p loom-server -- serve` |
| B | — | 仅环境变量，0 代码改动 |

---

## 8. 验证步骤

### Step 1 — 编译

```powershell
cd C:\Users\heycj\dev\worktrees\loom\cli-server-backend
cargo build -p loom-server
```

### Step 2 — 直接验证 loom-server

```powershell
Invoke-RestMethod -Uri "http://localhost:18081/provider"
Invoke-RestMethod -Uri "http://localhost:18081/config/providers"
Invoke-RestMethod -Uri "http://localhost:18081/model"
```

### Step 3 — openchamber 联调

```powershell
$env:OPENCODE_HOST = "http://127.0.0.1:18081"
$env:OPENCODE_SKIP_START = "true"
cd C:\Users\heycj\dev\openchamber-feat-dev
bun run packages/web/dev
```

控制台应显示：`Using external OpenCode server at http://127.0.0.1:18081 (skip-start mode)`

### Step 4 — 浏览器验证

打开 `http://localhost:3000`，检查：
1. Provider 列表非空
2. 模型选择下拉框有数据
3. "Add Provider" 显示 API Key 表单
4. 创建 session → 发送 prompt → SSE 流正常

---

## 9. 后续工作

### Phase 1 (P0): 当前改动 — Provider/Model 选择可用

### Phase 2 (P0): FS 完整化
- 新增 `/api/fs/{mkdir,stat,read,write,delete,rename,reveal,raw}` 路由
- 新增 `/api/config/settings` GET/PUT + `/api/config/reload` POST

### Phase 3 (P0): Git 基础
- 新增 `handlers/git.rs`，用 `std::process::Command` 调 git CLI
- 实现 check/status/diff/stage/unstage/commit/log/branches
- 注册到 `/api/git/*` 路由

### Phase 4 (P1): Terminal
- 使用 `portable-pty` 或 `tokio::process` 创建 PTY
- 实现 create/input/resize/close/stream 端点 + WebSocket

### Phase 5 (P1): Git 进阶
- push/pull/fetch, worktrees, rebase/merge/stash, identities

### Phase 6 (P2): GitHub / Push / Client Auth / Small Model / Dictation
- 按需实现

---

## 10. 文件参考

| 前端文件 | 内容 |
|---------|------|
| `packages/ui/src/lib/api/types.ts` | RuntimeAPIs 接口定义 |
| `packages/ui/src/lib/opencode/client.ts` | SDK client + baseUrl 配置 |
| `packages/ui/src/lib/gitApiHttp.ts` | Git API ~55 函数 |
| `packages/ui/src/lib/terminalApi.ts` | Terminal API |
| `packages/web/server/lib/opencode/env-config.js` | 环境变量解析 |
| `packages/web/server/lib/opencode/lifecycle.js` | opencode 进程生命周期 |
| `packages/web/server/lib/opencode/proxy.js` | 代理层 + pathRewrite |
| `packages/web/server/lib/opencode/routes.js` | openchamber 自有路由拦截 |
| `packages/web/server/lib/opencode/network-runtime.js` | 健康检查 + URL 构造 |
| `packages/web/server/lib/opencode/auth-state-runtime.js` | Basic Auth header |
| `.opencode/node_modules/@opencode-ai/sdk/dist/v2/gen/sdk.gen.js` | SDK 端点定义 |

| 后端文件 | 内容 |
|---------|------|
| `apps/server/src/routes.rs` | 全部 HTTP 路由注册 |
| `apps/server/src/handlers/bootstrap.rs` | v1+v2 bootstrap handler |
| `apps/server/src/handlers/provider.rs` | v2 Provider.Info handler |
| `apps/server/src/handlers/provider_auth.rs` | Provider auth（当前空 stub） |
| `apps/server/src/state.rs` | AppState 定义 |
| `apps/server/src/sse.rs` | SSE 事件流 |
