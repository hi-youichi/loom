# Loom 作为 OpenChamber 后端的对接方案

> **目标**: 用 `loom-server` 替换 OpenCode 成为 OpenChamber 的唯一后端，
> 前端零改动（或最小改动）即可正常工作。

## 设计决策

经代码审计和讨论确认以下决策：

| 决策项 | 选择 | 理由 |
|--------|------|------|
| 适配层位置 | **Loom 侧输出 OpenCode 格式** | 既然目标是替换 OpenCode，兼容层集中在 Loom 比散在 Express 更可控 |
| OpenChamber 前端 | **零改动** | OpenChamber 是独立项目，不因 Loom 而改前端 |
| `/config/providers` 格式 | **Loom 直接输出 OpenCode 结构** | Loom 负责 schema 兼容 |
| Provider auth 存储 | **全部 proxy 给 Loom，Express 不碰** | config.toml 归 Loom 管，单一职责 |
| 配置 reload | **SIGHUP 热加载** | 不重启进程，session 不丢失 |
| SSE 事件格式 | **Loom 已兼容 ✅** | translator.rs 已实现 Loom → OpenCode 事件转换 |

## 当前架构（OpenChamber + OpenCode）

```
浏览器 (:3000)
  │  Vite dev proxy /api/* → Express(:3001)
  │
  └─ Express Server (:3001)
       │
       ├─ 自行处理的路由（不 proxy）
       │   ├─ GET    /api/config/settings        ← 读磁盘 settings.json
       │   ├─ PUT    /api/config/settings        ← 写磁盘 settings.json
       │   ├─ GET    /api/config/opencode-resolution
       │   ├─ GET    /api/provider/:id/source    ← 从配置目录读 provider 文件
       │   ├─ DELETE /api/provider/:id/auth      ← 删 ~/.local/share/opencode/auth.json
       │   ├─ GET    /api/opencode/health
       │   ├─ GET    /api/opencode/version
       │   ├─ POST   /api/opencode/upgrade
       │   ├─ GET    /api/opencode/upgrade-status
       │   ├─ POST   /api/opencode/directory
       │   ├─ GET    /api/behavior/agents-md
       │   ├─ PUT    /api/behavior/agents-md
       │   └─ MCP auth CRUD: /api/mcp/auth/pending
       │
       └─ Proxy 到 OpenCode (动态端口，当前 :4096)
            ├─ readiness gate: 等 OpenCode ready 后放行
            ├─ pathRewrite: ^/api → ""
            │   即 /api/config/providers → /config/providers
            ├─ SSE forward: /api/event + /api/global/event 用 fetch streaming（非 proxy middleware）
            │
            ├─ GET    /config/providers          ← OpenCode 原生
            ├─ GET    /provider/auth             ← OpenCode 原生
            ├─ GET    /global/health             ← health check
            ├─ POST   /session/:id/prompt        ← 对话
            ├─ GET    /session/:id/messages      ← 消息
            └─ ... 所有未被 Express 拦截的 /api/* 路由
```

### OpenCode 启动握手协议

```
1. Express spawn:  opencode serve --hostname 127.0.0.1 --port PORT
2. 子进程 stdout:  "opencode server listening on http://127.0.0.1:PORT"
3. Express 解析:  提取 URL → waitForReady(url)
4. Health check:  GET http://.../global/health → { "healthy": true }
5. 就绪:          setOpenCodePort(PORT)
```

源码位置（OpenChamber `packages/web/server/lib/opencode/`）:
- spawn: `lifecycle.js:277` — `spawn(binary, args, ...)`
- binary 选择: `lifecycle.js:241` — `process.env.OPENCODE_BINARY || 'opencode'`
- spawn 参数: `lifecycle.js:242` — `['serve', '--hostname', hostname, '--port', String(port)]`
- stdout 解析: `lifecycle.js:304` — 硬编码 `line.startsWith('opencode server listening')`
- health check: `network-runtime.js:49` — 硬编码 `/global/health`
- 就绪判断: `network-runtime.js:62` — `body?.healthy === true`
- proxy 注册: `proxy.js` — `createProxyMiddleware` + `pathRewrite: { '^/api': '' }`
- SSE forward: `proxy.js` — `forwardSseRequest` 用 fetch streaming 而非 proxy middleware

### 认证机制

```
Express 启动时:
  1. ensureLocalOpenCodeServerPassword() → 生成随机密码
  2. 注入 OPENCODE_SERVER_PASSWORD 到子进程环境 (lifecycle.js:497)
  3. getOpenCodeAuthHeaders() 返回 { Authorization: "Basic base64(opencode:password)" }

Proxy 请求时:
  4. proxy.js onProxyReq: proxyReq.setHeader('Authorization', authHeaders.Authorization)
  5. forwardSseRequest: headers 注入 getOpenCodeAuthHeaders()
```

源码位置:
- 密码生成: `lifecycle.js:476` — `ensureLocalOpenCodeServerPassword({ rotateManaged: true })`
- 密码注入: `lifecycle.js:497` — `OPENCODE_SERVER_PASSWORD: openCodePassword`
- auth headers: `auth-state-runtime.js:47-56` — Basic auth, username=`opencode`
- proxy 注入: `proxy.js` — `proxyReq.setHeader('Authorization', ...)`

### Express 拦截的路由（不走 proxy）

以下路由由 Express **直接处理**，永远不会到达后端:

| 方法 | 路由 | 处理方式 | 源码 |
|------|------|---------|------|
| GET | `/api/config/settings` | 读磁盘 `~/.config/openchamber/settings.json` | `routes.js:135` |
| PUT | `/api/config/settings` | 写磁盘 `settings.json` | `routes.js:296` |
| GET | `/api/config/opencode-resolution` | 查 OpenCode 二进制路径 | `routes.js:140` |
| GET | `/api/provider/:id/source` | 从配置目录读 provider 文件 + auth.js 查 auth | `routes.js:377` |
| DELETE | `/api/provider/:id/auth` | 删除 `~/.local/share/opencode/auth.json` 中对应条目 | `routes.js:413` |
| GET | `/api/opencode/health` | proxy 到 `/global/health` | `routes.js` |
| GET | `/api/opencode/version` | proxy 到 `/global/health` 取 version | `routes.js` |
| GET | `/api/opencode/upgrade-status` | 查 npm/github 最新版本 | `routes.js` |
| POST | `/api/opencode/directory` | 写 settings.json 的 lastDirectory | `routes.js:475` |
| GET/PUT | `/api/behavior/agents-md` | 读写 AGENTS.md | `routes.js:530+` |

### Provider Auth 原始流程（OpenCode + OpenChamber）

```
连接 Provider:
  1. GET /api/config/providers   → proxy → OpenCode 返回 provider 列表
  2. GET /api/provider/auth      → proxy → OpenCode 返回各 provider 的认证方式
  3. 用户选择 provider + 输入 API Key
  4. 前端通过 proxy 写入 → OpenCode 自己写入 ~/.local/share/opencode/auth.json
  5. OpenCode 内部 reload（或重启时读取）

断开 Provider:
  6. DELETE /api/provider/:id/auth → Express 直接处理
     ├─ auth.js:removeProviderAuth() → 读/写 auth.json
     └─ refreshOpenCodeAfterConfigChange → kill + respawn 子进程
  7. GET /api/provider/:id/source → Express 直接处理
     ├─ getProviderSources() → 从配置目录读 provider 文件
     └─ auth.js:getProviderAuth() → 查 auth.json 是否已连接
```

关键文件（OpenChamber 侧）:
- `auth.js` — 读写 `~/.local/share/opencode/auth.json`，被 Express 的 DELETE/SOURCE handler 调用
- `routes.js:413-471` — DELETE provider auth handler
- `routes.js:377-411` — GET provider source handler
- `lifecycle.js` — `refreshOpenCodeAfterConfigChange()` 是 kill + respawn，非热加载

### Proxy 转发的路由（到达后端）

以下路由经过 `pathRewrite: ^/api → ""` 后转发到后端:

| 前端请求 | 后端收到 | 用途 |
|---------|---------|------|
| `GET /api/config/providers` | `GET /config/providers` | provider 列表 |
| `GET /api/provider/auth` | `GET /provider/auth` | provider 认证方式 |
| `GET /api/session` | `GET /session` | session 列表 |
| `POST /api/session/:id/prompt` | `POST /session/:id/prompt` | 发送消息 |
| `GET /api/session/:id/messages` | `GET /session/:id/messages` | 消息列表 |
| `GET /api/event` | `GET /event` | SSE 事件流（fetch streaming） |
| `GET /api/global/event` | `GET /global/event` | SSE 事件流（fetch streaming） |

**注意**: readiness gate 中间件 (`proxy.js`) 白名单了部分路径直接 `next()`:
```
/api/themes/custom
/api/push
/api/config/settings
/api/config/agents
/api/config/opencode-resolution
/api/config/skills
/api/config/reload
/api/health
```

---

## 方案: Loom 兼容层 + Express 配合

### 核心思路

1. **Loom 侧**: Loom 输出 OpenCode 兼容的数据格式和协议，使 Express proxy 自然工作
2. **OpenChamber 侧**: 修改 `lifecycle.js` 的 spawn 逻辑，用 `loom-server` 替换 `opencode`
3. **Provider auth 全部 proxy 给 Loom**，Express 不碰 provider auth 数据

### 改动总览

```
┌─ Loom 后端 ──────────────────────────────────────────────┐
│                                                          │
│  C-1  stdout 输出 "opencode server listening on ..."      │
│  C-2  --hostname 作为 --host 的 visible_alias             │
│  C-3  auth.rs 兼容 OPENCODE_SERVER_PASSWORD (Basic auth)  │
│  C-4  GET /provider/auth 返回 API key 认证方式            │
│  C-5  POST /provider/:id/auth 写入 config.toml            │
│  C-6  GET /config/providers 输出 OpenCode 格式            │
│  C-7  GET /provider/:id/source 返回连接状态               │
│  C-8  DELETE /provider/:id/auth 删除 config.toml 条目     │
│  C-9  SIGHUP 热加载 config.toml                           │
│  C-10 GET /global/health 返回 version 字段                │
│                                                          │
│  已有 ✅: /global/health, SSE v1+v2, translator.rs        │
│                                                          │
└──────────────────────────────────────────────────────────┘

┌─ OpenChamber ────────────────────────────────────────────┐
│                                                          │
│  B-1  设置 OPENCODE_BINARY=loom-server                    │
│  B-2  DELETE /api/provider/:id/auth 改为 proxy 给 Loom    │
│  B-3  GET /api/provider/:id/source 改为 proxy 给 Loom     │
│                                                          │
│  不改: 前端 UI、settings.json、Express proxy 逻辑         │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

---

## Loom 后端改动

### C-1. stdout 启动消息

**当前** (`main.rs:75`):
```rust
println!("loom-server listening on http://{bound}");
```

**改为**:
```rust
println!("opencode server listening on http://{bound}");
```

OpenChamber `lifecycle.js:304` 硬编码匹配 `line.startsWith('opencode server listening')`。
不改这行 → 30 秒超时报错，Express 认为启动失败。

### C-2. `--hostname` CLI alias

OpenChamber spawn 传入 `--hostname`（`lifecycle.js:242`），Loom 当前只接受 `--host`。

当前 (`main.rs:42`):
```rust
#[arg(long, default_value = "127.0.0.1")]
host: String,
```

改为:
```rust
#[arg(long = "host", visible_alias = "hostname", default_value = "127.0.0.1")]
host: String,
```

`serve` 子命令的参数也需要同步修改。

### C-3. `OPENCODE_SERVER_PASSWORD` 兼容

OpenChamber 认证链路:
```
1. lifecycle.js:476  生成随机密码
2. lifecycle.js:497  注入 OPENCODE_SERVER_PASSWORD 到子进程环境
3. auth-state-runtime.js:47-56  getOpenCodeAuthHeaders() 返回 Basic auth
   ├─ username: process.env.OPENCODE_SERVER_USERNAME || 'opencode'
   ├─ password: OPENCODE_SERVER_PASSWORD
   └─ 格式: { Authorization: "Basic base64(opencode:password)" }
4. proxy.js  proxyReq.setHeader('Authorization', ...)
```

Loom auth middleware (`auth.rs`) 当前检查 `LOOM_AUTH_TOKEN`，需要:
- 读取 `OPENCODE_SERVER_PASSWORD` 环境变量
- 支持 Basic auth 校验（`opencode:password`）
- 与 `LOOM_AUTH_TOKEN` 并行支持，任一匹配即可

### C-4. `GET /provider/auth`（新增）

OpenCode 在 `GET /provider/auth` 返回各 provider 支持的认证方式。

Loom 的 provider 全部使用 API Key 认证（非 OAuth），返回值:
```json
{
  "minimax-cn-coding-plan": [
    { "type": "api", "label": "Manually enter API Key" }
  ],
  "zhipuai-coding-plan": [
    { "type": "api", "label": "Manually enter API Key" }
  ]
}
```

实现: 从 `~/.loom/config.toml` 读取已配置的 providers，每个返回 `{ type: "api", label: "Manually enter API Key" }`。

当前状态: `handlers/provider_auth.rs` 已有文件但内容被清空（W4 cleanup），需要重新实现。

### C-5. `POST /provider/:id/auth` 写入 config.toml（新增）

前端 "Connect Provider" 流程中，用户输入 API Key 后需要一个写入端点。

```json
POST /provider/:providerId/auth
Content-Type: application/json

{ "apikey": "sk-xxx" }
```

Loom 将 API Key 写入 `~/.loom/config.toml` 对应 provider 的配置中。

### C-6. `/config/providers` 格式适配

**当前 Loom 返回**:
```json
{
  "providers": [
    { "id": "minimax-cn-coding-plan", "name": "minimax-cn-coding-plan", "models": [...], "default_model": "MiniMax-M3" }
  ],
  "default": { "provider": "minimax-cn-coding-plan", "model": "MiniMax-M3" }
}
```

**OpenCode 返回**（前端期望）:
```json
{
  "providers": [
    {
      "id": "zhipuai-coding-plan",
      "name": "Zhipu AI Coding Plan",
      "source": "api",
      "env": ["ZHIPU_API_KEY"],
      "key": "xxx",
      "options": {},
      "models": {
        "glm-5v-turbo": { "id": "glm-5v-turbo", "providerID": "zhipuai-coding-plan", "name": "GLM-5V-Turbo", ... }
      }
    }
  ],
  "default": { "zhipuai-coding-plan": "glm-5v-turbo" }
}
```

**关键差异**:
- `default` 格式: Loom 用 `{ provider, model }`，OpenCode 用 `{ providerId: modelId }` 映射表
- `models`: Loom 用字符串数组，OpenCode 用对象映射（key=modelId）
- `source`/`env`/`key`/`options`: OpenCode 额外字段

**改动**: 调整 `get_config_providers()` 返回格式:
- `default` 改为 `{ providerId: modelId }` 映射
- `models` 改为对象 `{ modelId: { id, providerID, name, ... } }`
- 每个 provider 添加 `source: "api"` 和 `env: ["API_KEY"]` 字段

### C-7. `GET /provider/:id/source` 返回连接状态（新增）

当前 Express `routes.js:377-411` 直接处理此路由（读配置目录 + auth.js）。
需要让此请求 proxy 给 Loom。

Loom 返回:
```json
{
  "providerId": "minimax-cn-coding-plan",
  "sources": {
    "auth": { "exists": true }
  }
}
```

`auth.exists` 取决于 config.toml 中该 provider 是否有 API Key。

### C-8. `DELETE /provider/:id/auth` 删除 config.toml 条目（新增）

当前 Express `routes.js:413-471` 直接处理此路由（删 auth.json + 重启）。
需要让此请求 proxy 给 Loom。

Loom 收到后:
1. 从 config.toml 删除对应 provider 的 API Key
2. 热加载 config（C-9）

### C-9. SIGHUP 热加载 config.toml（新增）

收到 SIGHUP 信号后:
1. 重新读取 `~/.loom/config.toml`
2. 更新 provider 列表
3. 不中断现有 session

OpenChamber 的 `refreshOpenCodeAfterConfigChange` 当前是 kill + respawn。
替换 Loom 后，Express 的 `refreshOpenCodeAfterConfigChange` 改为发送 SIGHUP（OpenChamber 侧改动）。

### C-10. `/global/health` version 字段（已有 ✅）

Loom 已返回 `version` 字段:
```json
{ "healthy": true, "ok": true, "kind": "external-kernel", "version": "0.4.0" }
```

满足 OpenChamber `routes.js` 中 `readOpenCodeCurrentVersion()` 的版本检测。

---

## SSE 兼容性（已有 ✅）

Loom SSE 实现已兼容 OpenCode 格式，无需改动:

| 方面 | Loom 实现 | 源码 | 兼容 |
|------|-----------|------|------|
| V1 envelope | `{ directory, payload: { type, properties } }` | `sse.rs:206-218` | ✅ |
| V2 envelope | `{ id, type, data, location }` | `sse.rs:223` | ✅ |
| V1 endpoint | `GET /global/event` | `sse.rs:40` | ✅ |
| V2 endpoint | `GET /api/event` | `sse.rs:48` | ✅ |
| session SSE | `GET /api/session/:id/event` | `sse.rs:56` | ✅ |
| 心跳 | 10s business event + keepalive comment | `sse.rs:37,98-105` | ✅ |
| 事件转换 | Loom → OpenCode (message.part.updated, message.tokens, session.status) | `translator.rs` | ✅ |

事件类型映射（translator.rs）:
- `Messages { kind: Message }` → `message.part.updated` (text part)
- `Messages { kind: Thinking }` → `message.part.updated` (reasoning part)
- `TaskStart` → `message.part.updated` (tool part, status=pending)
- `TaskEnd { Ok }` → `message.part.updated` (tool part, status=completed)
- `TaskEnd { Err }` → `message.part.updated` (tool part, status=error)
- `Usage` → `message.tokens`

Express SSE proxy（`proxy.js:forwardSseRequest`）用 fetch streaming 转发，透明传输。

---

## Session 持久化（已有 ✅）

Loom 已有 session 持久化到磁盘（`storage.rs`），重启后 session 不丢失。
OpenChamber 前端刷新页面后 `GET /api/session` 能正常拉取历史 session。

---

## OpenChamber 前端改动

### B-1. 设置 `OPENCODE_BINARY=loom-server`

启动脚本或环境变量设置:
```bash
OPENCODE_BINARY=loom-server bun run dev:server
```

`lifecycle.js:241` 会读取此环境变量:
```js
let binary = (process.env.OPENCODE_BINARY || 'opencode').trim() || 'opencode';
```

不需要改 lifecycle.js 代码，只需设环境变量。

### B-2. `DELETE /api/provider/:id/auth` 改为 proxy 给 Loom

当前 `routes.js:413-471` 直接处理此路由（调 auth.js 删 auth.json + refreshOpenCodeAfterConfigChange）。

改为: 注释掉或删除 `routes.js` 中的 `app.delete('/api/provider/:providerId/auth', ...)` handler，
让请求走到 proxy → Loom `/provider/:id/auth`。

### B-3. `GET /api/provider/:id/source` 改为 proxy 给 Loom

当前 `routes.js:377-411` 直接处理此路由。

改为: 注释掉或删除 `routes.js` 中的 `app.get('/api/provider/:providerId/source', ...)` handler，
让请求走到 proxy → Loom `/provider/:id/source`。

---

## 实现优先级

### Phase 0: 启动握手（能 spawn、health check 通过）

| # | 改动侧 | 任务 | 文件 | 复杂度 |
|---|--------|------|------|--------|
| 1 | Loom | C-1 stdout 改为 `opencode server listening on ...` | `apps/server/src/main.rs` | 1 行 |
| 2 | Loom | C-2 `--hostname` 作为 `--host` 的 visible_alias | `apps/server/src/main.rs` | 1 行 |
| 3 | Loom | C-3 `OPENCODE_SERVER_PASSWORD` Basic auth 兼容 | `apps/server/src/auth.rs` | ~20 行 |
| 4 | OpenChamber | B-1 设置 `OPENCODE_BINARY=loom-server` | 环境变量 | 配置 |

**验收**: `OPENCODE_BINARY=loom-server bun run dev:server` → Express 成功 spawn loom-server，health check 通过

### Phase 1: Provider 可用（能选 provider、输入 key、看到列表）

| # | 改动侧 | 任务 | 文件 | 复杂度 |
|---|--------|------|------|--------|
| 5 | Loom | C-6 `/config/providers` 输出 OpenCode 格式 | `apps/server/src/handlers/bootstrap.rs` | ~50 行 |
| 6 | Loom | C-4 `GET /provider/auth` 返回认证方式 | `apps/server/src/handlers/provider_auth.rs` | ~30 行 |
| 7 | Loom | C-5 `POST /provider/:id/auth` 写入 config.toml | `apps/server/src/handlers/provider_auth.rs` | ~40 行 |
| 8 | OpenChamber | B-2 注释 DELETE provider auth handler | `packages/web/server/lib/opencode/routes.js` | 删代码 |
| 9 | OpenChamber | B-3 注释 GET provider source handler | `packages/web/server/lib/opencode/routes.js` | 删代码 |

**验收**: 前端 Settings → Providers 页面显示 Loom provider；点击 Connect Provider → 输入 API Key → 成功连接

### Phase 2: 完整体验（对话、断开、热加载）

| # | 改动侧 | 任务 | 文件 | 复杂度 |
|---|--------|------|------|--------|
| 10 | Loom | C-7 `GET /provider/:id/source` 返回连接状态 | `apps/server/src/handlers/provider_auth.rs` | ~20 行 |
| 11 | Loom | C-8 `DELETE /provider/:id/auth` 删除 config 条目 | `apps/server/src/handlers/provider_auth.rs` | ~30 行 |
| 12 | Loom | C-9 SIGHUP 热加载 config.toml | `apps/server/src/main.rs` 或信号处理 | ~60 行 |
| 13 | OpenChamber | `refreshOpenCodeAfterConfigChange` 改为 SIGHUP | `packages/web/server/lib/opencode/lifecycle.js` | ~10 行 |

**验收**: 能对话（SSE 流式返回）；能断开 Provider（DELETE 后 config.toml 更新）；改配置后热加载不重启

---

## 验收标准

1. **启动**: `OPENCODE_BINARY=loom-server bun run dev:server` → Express 成功 spawn loom-server，health check 通过
2. **Provider 列表**: 前端 Settings → Providers 页面显示 Loom config.toml 中的 provider
3. **Connect Provider**: 点击 "Connect Provider" → 弹出 provider 选择对话框 → 输入 API Key → 成功连接
4. **对话**: 前端发送消息 → Loom 处理 → SSE 流式返回
5. **Disconnect Provider**: 断开 Provider → config.toml 更新 → 列表刷新
6. **Settings**: 前端 Settings 页面显示和修改配置（settings.json 由 Express 管）
7. **热加载**: 修改 config.toml 后 SIGHUP → provider 列表更新，session 不中断

---

## 风险与注意事项

- **Express settings.json vs Loom config.toml**: 明确分工 — settings.json 存前端 UI 偏好（主题、快捷键），config.toml 存 provider 和 Loom 配置
- **Provider auth 数据统一**: 全部走 Loom config.toml，Express 不碰 auth.json
- **Auth 认证**: OpenChamber Express 注入 `OPENCODE_SERVER_PASSWORD`，使用 Basic auth（`opencode:password`），Loom auth.rs 需兼容此格式
- **Windows path**: OpenChamber 在 Windows 上有 session merge 逻辑（`proxy.js` 中 `process.platform === 'win32'` 分支），Loom 的 session 持久化需与此兼容
- **SSE backpressure**: OpenChamber proxy.js 实现了 SSE backpressure（`writeSseChunkWithBackpressure`），Loom SSE 输出需保持稳定流
