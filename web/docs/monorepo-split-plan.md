# Graphweave Web — Monorepo 拆分方案（功能细化版）

## 概述

将当前单 Vite + React 应用拆分为 11 个功能包 + 1 个应用壳，基于 pnpm workspace + Turborepo。

## 目录结构

```
graphweave-web/
├── package.json
├── pnpm-workspace.yaml
├── turbo.json
├── tsconfig.json
├── packages/
│   ├── protocol/          # @graphweave/protocol
│   ├── types/             # @graphweave/types
│   ├── utils/             # @graphweave/utils
│   ├── adapters/          # @graphweave/adapters
│   ├── ws-client/         # @graphweave/ws-client
│   ├── service-session/   # @graphweave/service-session
│   ├── service-workspace/ # @graphweave/service-workspace
│   ├── service-agent/     # @graphweave/service-agent
│   ├── service-chat/      # @graphweave/service-chat
│   ├── hooks/             # @graphweave/hooks
│   ├── ui/                # @graphweave/ui
│   └── app/               # @graphweave/app
├── .storybook/
└── docs/
```

## 依赖图

```
                    app
                 ───┼─────
               ┌────┼────┐
               ▼         ▼
             hooks      ui
            ───┼───      │
          ┌────┼────┐    │
          ▼    ▼    ▼    ▼
    svc-chat svc-*  │  types
          │    │    │    ▲
          ▼    ▼    │    │
       ws-client   │  utils
          │        │    ▲
          ▼        ▼────┤
       protocol ───────►│
                         │
       adapters ─────────┘

service-session / service-workspace / service-agent
      各自依赖 → ws-client, types
```

简化依赖层级：

| 层级 | 包 | 可依赖 |
|------|-----|--------|
| L0 | `protocol` | 无 |
| L0 | `types` | `protocol` |
| L0 | `utils` | `types` |
| L1 | `adapters` | `types` |
| L1 | `ws-client` | `protocol` |
| L2 | `service-session` | `ws-client`, `types` |
| L2 | `service-workspace` | `ws-client`, `types` |
| L2 | `service-agent` | `ws-client`, `types` |
| L2 | `service-chat` | `ws-client`, `types`, `adapters` |
| L3 | `hooks` | 所有 `service-*`, `types`, `adapters` |
| L3 | `ui` | `types` |
| L4 | `app` | `hooks`, `ui`, `types` |

---

## 各包详细说明

---

### `@graphweave/protocol`

**职责：** Loom 协议类型定义，纯类型包，零运行时代码。

**源码：**

| 文件 | 来自 |
|------|------|
| `src/loom.ts` | `src/types/protocol/loom.ts` |

**package.json 关键字段：**

```json
{
  "name": "@graphweave/protocol",
  "main": "./dist/index.js",
  "types": "./dist/index.d.ts",
  "scripts": {
    "build": "tsup src/index.ts --format esm,cjs --dts"
  }
}
```

---

### `@graphweave/types`

**职责：** UI/业务实体类型定义。

**源码：**

| 文件 | 来自 |
|------|------|
| `src/agent.ts` | `src/types/agent.ts` |
| `src/chat.ts` | `src/types/chat.ts` |
| `src/icons.ts` | `src/types/icons.ts` |
| `src/session.ts` | `src/types/session.ts` |
| `src/toolConfig.ts` | `src/types/toolConfig.ts` |
| `src/ui/message.ts` | `src/types/ui/message.ts` |
| `src/workspace.ts` | `src/types/workspace.ts` (从 protocol/loom.ts 中提取 WorkspaceMeta 等类型) |

**依赖：** `@graphweave/protocol`

---

### `@graphweave/utils`

**职责：** 通用工具函数，无业务逻辑依赖。

**源码：**

| 文件 | 来自 |
|------|------|
| `src/format.ts` | `src/utils/format.ts` |
| `src/toolTitle.ts` | `src/utils/toolTitle.ts` |

**依赖：** 无（或仅 `@graphweave/types` 若工具函数引用了类型）

---

### `@graphweave/adapters`

**职责：** 数据适配器，将协议层原始数据转换为 UI 层可用格式。

**源码：**

| 文件 | 来自 |
|------|------|
| `src/MessageAdapter.ts` | `src/adapters/MessageAdapter.ts` |
| `src/ToolBlockAdapter.ts` | `src/adapters/ToolBlockAdapter.ts` |
| `src/ToolStreamAggregator.ts` | `src/adapters/ToolStreamAggregator.ts` |

**依赖：** `@graphweave/types`

---

### `@graphweave/ws-client`

**职责：** WebSocket 连接管理、消息收发、重连逻辑。与具体业务无关的传输层。

**源码：**

| 文件 | 来自 |
|------|------|
| `src/connection.ts` | `src/services/connection.ts`（提取连接管理部分） |
| `src/types.ts` | `Model` 类型、`WebSocketStatus` 等 |

**依赖：** `@graphweave/protocol`

**说明：** 将 `connection.ts` 中的环境变量读取、WebSocket 管理、request/response 封装抽离为独立包。`listModels`/`setSessionModel` 移入对应的 service 包。

---

### `@graphweave/service-session`

**职责：** Session 的 CRUD、localStorage 持久化。

**源码：**

| 文件 | 来自 |
|------|------|
| `src/session.ts` | `src/services/session.ts` |

**依赖：** `@graphweave/ws-client`, `@graphweave/types`

**对应 Hooks：** `useSessions`, `useRealtimeSessions`, `useSessionId`

---

### `@graphweave/service-workspace`

**职责：** Workspace 管理（创建/删除/切换/会话绑定）。

**源码：**

| 文件 | 来自 |
|------|------|
| `src/workspace.ts` | `src/services/workspace.ts` |

**依赖：** `@graphweave/ws-client`, `@graphweave/types`

**对应 Hooks：** `useWorkspace`

---

### `@graphweave/service-agent`

**职责：** Agent 列表获取、Agent 模型映射持久化。

**源码：**

| 文件 | 来自 |
|------|------|
| `src/agent.ts` | `src/services/agent.ts` |

**依赖：** `@graphweave/ws-client`, `@graphweave/types`

**对应 Hooks：** `useAgents`, `useAgentModel`

---

### `@graphweave/service-chat`

**职责：** 聊天消息发送、流式响应处理、用户历史消息获取。

**源码：**

| 文件 | 来自 |
|------|------|
| `src/chat.ts` | `src/services/chat.ts` |
| `src/userMessages.ts` | `src/services/userMessages.ts` |
| `src/model.ts` | `src/services/model.ts` |

**依赖：** `@graphweave/ws-client`, `@graphweave/types`, `@graphweave/adapters`

**说明：** `model.ts` 中的 `listModels`/`setSessionModel` 与聊天场景强关联，归入此包。若后续模型选择独立为其他功能所用，可再拆出 `service-model`。

**对应 Hooks：** `useChat`, `useMessages`, `useModels`

---

### `@graphweave/hooks`

**职责：** 所有 React Hooks，整合 services 提供 React 友好的状态管理接口。

**源码：**

| 文件 | 来自 |
|------|------|
| `src/useChat.ts` | `src/hooks/useChat.ts` |
| `src/useChatPanel.ts` | `src/hooks/useChatPanel.ts` |
| `src/useMessages.ts` | `src/hooks/useMessages.ts` |
| `src/useAgents.ts` | `src/hooks/useAgents.ts` |
| `src/useAgentModel.ts` | `src/hooks/useAgentModel.ts` |
| `src/useModels.ts` | `src/hooks/useModels.ts` |
| `src/useSessions.ts` | `src/hooks/useSessions.ts` |
| `src/useRealtimeSessions.ts` | `src/hooks/useRealtimeSessions.ts` |
| `src/useSessionId.ts` | `src/hooks/useSessionId.ts` |
| `src/useWorkspace.ts` | `src/hooks/useWorkspace.ts` |
| `src/useWebSocket.ts` | `src/hooks/useWebSocket.ts` |
| `src/useTheme.tsx` | `src/hooks/useTheme.tsx` |
| `src/themeUtils.ts` | `src/hooks/themeUtils.ts` |

**依赖：** 所有 `@graphweave/service-*`, `@graphweave/types`, `@graphweave/adapters`

**peerDependencies：** `react`, `react-dom`

---

### `@graphweave/ui`

**职责：** 纯展示组件库，按功能域组织。

**源码（保持原有组件目录结构不变）：**

| 目录 | 组件 |
|------|------|
| `src/chat/` | AgentChatSidebar, MarkdownContent, MessageItem, MessageList, TextMessage |
| `src/dashboard/` | ActivityFeed, AgentCard, AgentGrid, DashboardView |
| `src/file-tree/` | FileTree, FileTreeContext, FileTreeItem, FileTreeSidebar, useFileTree |
| `src/sessions/` | SessionCard, SessionList |
| `src/tabs/` | TabContent, TabNavigator |
| `src/workspace/` | WorkspaceSelector |
| `src/ui/` | popover (基础 UI 原子组件) |
| `src/` | MessageComposer, ModelSelector, ThemeToggle, ToolCard, ToolIcon |
| `src/styles/` | sessions.css, streamingIndicator.css, tabs.css, toolComponents.css |

**依赖：** `@graphweave/types`

**peerDependencies：** `react`, `react-dom`

**第三方依赖：** `react-markdown`, `rehype-highlight`, `remark-gfm`, `lucide-react`, `class-variance-authority`, `clsx`, `tailwind-merge`, `radix-ui`, `@base-ui/react`

---

### `@graphweave/app`

**职责：** 应用入口壳，路由编排，全局样式，E2E 测试。

**源码：**

| 文件 | 来自 |
|------|------|
| `src/App.tsx` | `src/App.tsx` |
| `src/main.tsx` | `src/main.tsx` |
| `index.html` | `index.html` |
| `src/index.css` | `src/index.css` |
| `e2e/` | `e2e/` |

**依赖：** `@graphweave/hooks`, `@graphweave/ui`, `@graphweave/types`

---

## 根配置文件

### `pnpm-workspace.yaml`

```yaml
packages:
  - "packages/*"
```

### `turbo.json`

```json
{
  "$schema": "https://turbo.build/schema.json",
  "tasks": {
    "build": {
      "dependsOn": ["^build"],
      "outputs": ["dist/**"]
    },
    "dev": {
      "cache": false,
      "persistent": true
    },
    "test": {
      "dependsOn": ["build"]
    },
    "lint": {}
  }
}
```

### 根 `tsconfig.json`

```json
{
  "files": [],
  "references": [
    { "path": "packages/protocol" },
    { "path": "packages/types" },
    { "path": "packages/utils" },
    { "path": "packages/adapters" },
    { "path": "packages/ws-client" },
    { "path": "packages/service-session" },
    { "path": "packages/service-workspace" },
    { "path": "packages/service-agent" },
    { "path": "packages/service-chat" },
    { "path": "packages/hooks" },
    { "path": "packages/ui" },
    { "path": "packages/app" }
  ]
}
```

---

## 迁移步骤

### 阶段 1：初始化 workspace

1. 安装 pnpm：`npm install -g pnpm`
2. 创建根 `pnpm-workspace.yaml`、`turbo.json`
3. 创建 `packages/` 下 12 个子目录
4. 各包初始化 `package.json`、`tsconfig.json`
5. `pnpm install` 验证 workspace

### 阶段 2：迁移 L0 基础包（无依赖或最小依赖）

1. **`protocol`** — 移入 `src/types/protocol/loom.ts`
2. **`types`** — 移入 `src/types/` 下其余文件
3. **`utils`** — 移入 `src/utils/format.ts`、`src/utils/toolTitle.ts`
4. 逐一构建验证：`pnpm --filter @graphweave/{protocol,types,utils} build`

### 阶段 3：迁移 L1 适配层

1. **`adapters`** — 移入 `src/adapters/`
2. **`ws-client`** — 从 `src/services/connection.ts` 提取连接管理核心
3. 迁移对应测试：`__tests__/adapters/`
4. 构建验证

### 阶段 4：迁移 L2 业务服务层

1. **`service-session`** — 移入 `src/services/session.ts`
2. **`service-workspace`** — 移入 `src/services/workspace.ts`
3. **`service-agent`** — 移入 `src/services/agent.ts`
4. **`service-chat`** — 移入 `src/services/chat.ts`、`userMessages.ts`、`model.ts`
5. 迁移对应测试：`__tests__/services/`
6. 构建验证

### 阶段 5：迁移 L3 表现层

1. **`hooks`** — 移入 `src/hooks/`，更新所有 import 路径
2. **`ui`** — 移入 `src/components/`、`src/styles/`
3. 迁移测试：`__tests__/hooks/`、`__tests__/components/`
4. 迁移 Storybook 配置指向 `packages/ui`
5. 构建验证

### 阶段 6：迁移 L4 应用壳

1. **`app`** — 移入 `App.tsx`、`main.tsx`、`index.html`、`index.css`
2. 移入 `e2e/` 目录及 `playwright.config.ts`
3. 更新所有导入为 `@graphweave/*` 包引用
4. 全量构建 + E2E 测试验证

### 阶段 7：收尾

1. 删除旧 `src/` 目录
2. 更新 `.gitignore`（各包 `dist/`）
3. 更新 CI/CD 管道（利用 Turborepo 缓存）
4. 更新项目 README.md

---

## 工具链选型

| 用途 | 工具 | 理由 |
|------|------|------|
| 包管理 | pnpm | 原生 workspace、硬链接、严格依赖隔离 |
| 构建编排 | Turborepo | 增量构建、并行任务、远程缓存 |
| 包内构建 | tsup | esbuild 驱动、零配置 ESM+CJS+DTS |
| 单元测试 | Vitest | workspace 模式、各包独立配置 |
| E2E 测试 | Playwright | 保持在 app 包内 |
| 组件文档 | Storybook | 指向 ui 包 |
| Lint | ESLint | 配置提升到根目录 |

## 包规模一览

| 包 | 文件数 | 预估复杂度 |
|----|--------|-----------|
| `protocol` | 1 | 低 |
| `types` | 7 | 低 |
| `utils` | 2 | 低 |
| `adapters` | 3 | 中 |
| `ws-client` | 1 | 中 |
| `service-session` | 1 | 低 |
| `service-workspace` | 1 | 低 |
| `service-agent` | 1 | 低 |
| `service-chat` | 3 | 中 |
| `hooks` | 13 | 高 |
| `ui` | ~25 | 高 |
| `app` | 3 + e2e | 中 |

## 注意事项

- **tailwind 配置**：`ui` 包保留 `tailwind.config.js`，`app` 包负责最终样式组装
- **路径别名**：将现有 `@/` 替换为 `@graphweave/*` 包引用
- **共享 devDependencies**：ESLint、TypeScript、Vitest 配置提升到根 `package.json`
- **Storybook**：`.storybook/main.ts` 需配置 `@graphweave/ui` 的路径别名
- **版本策略**：初期所有包统一版本号 (`0.0.0`)，后续可按需独立发版
- **ws-client 通用化**：若未来需要 HTTP 请求，可在 `ws-client` 基础上扩展或新增 `http-client` 包
