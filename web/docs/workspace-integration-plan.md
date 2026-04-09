# Workspace 与 Web 前端整合方案

## 概览

后端 `loom-workspace` crate 提供 Workspace（工作空间）和 Thread（对话线程）的关联存储，服务端 `serve` 通过 WebSocket 暴露 6 个 workspace 相关请求/响应。当前 Web 前端尚无 workspace 相关的类型、服务层或 UI 组件。

本方案将 workspace 功能端到端整合进 Web 项目，使用户可以在界面上创建/切换工作空间、管理其下的对话线程。

---

## 1. 协议类型补全 — `src/types/protocol/loom.ts`

在现有 Loom 协议类型文件中新增 workspace 请求和响应类型，一一对应后端 Rust 定义。

### 1.1 请求类型

```ts
// ——— Workspace 请求 ———

export type WorkspaceListRequest = {
  type: 'workspace_list'
  id: string
}

export type WorkspaceCreateRequest = {
  type: 'workspace_create'
  id: string
  name?: string
}

export type WorkspaceThreadListRequest = {
  type: 'workspace_thread_list'
  id: string
  workspace_id: string
}

export type WorkspaceThreadAddRequest = {
  type: 'workspace_thread_add'
  id: string
  workspace_id: string
  thread_id: string
}

export type WorkspaceThreadRemoveRequest = {
  type: 'workspace_thread_remove'
  id: string
  workspace_id: string
  thread_id: string
}

export type WorkspaceRequest =
  | WorkspaceListRequest
  | WorkspaceCreateRequest
  | WorkspaceThreadListRequest
  | WorkspaceThreadAddRequest
  | WorkspaceThreadRemoveRequest
```

### 1.2 响应类型

```ts
export type WorkspaceMeta = {
  id: string
  name?: string | null
  created_at_ms: number
}

export type ThreadInWorkspace = {
  thread_id: string
  created_at_ms: number
}

export type WorkspaceListResponse = {
  type: 'workspace_list'
  id: string
  workspaces: WorkspaceMeta[]
}

export type WorkspaceCreateResponse = {
  type: 'workspace_create'
  id: string
  workspace_id: string
}

export type WorkspaceThreadListResponse = {
  type: 'workspace_thread_list'
  id: string
  workspace_id: string
  threads: ThreadInWorkspace[]
}

export type WorkspaceThreadAddResponse = {
  type: 'workspace_thread_add'
  id: string
  workspace_id: string
  thread_id: string
}

export type WorkspaceThreadRemoveResponse = {
  type: 'workspace_thread_remove'
  id: string
  workspace_id: string
  thread_id: string
}

export type WorkspaceResponse =
  | WorkspaceListResponse
  | WorkspaceCreateResponse
  | WorkspaceThreadListResponse
  | WorkspaceThreadAddResponse
  | WorkspaceThreadRemoveResponse
```

### 1.3 扩展 LoomServerMessage

在现有的 `LoomServerMessage` 联合类型中加入 workspace 响应变体：

```ts
export type LoomServerMessage =
  | LoomRunStreamEventResponse
  | LoomRunEndResponse
  | LoomErrorResponse
  | WorkspaceListResponse
  | WorkspaceCreateResponse
  | WorkspaceThreadListResponse
  | WorkspaceThreadAddResponse
  | WorkspaceThreadRemoveResponse
  | { type: string }
```

---

## 2. Workspace 服务层 — `src/services/workspace.ts`

参照现有 `services/chat.ts` 中 WebSocket 连接建立的模式，创建 workspace 服务层。核心思路：**复用同一条 WebSocket 连接**，通过 JSON `type` 字段区分请求类型。

### 文件结构

```
src/services/
  chat.ts          ← 已有，负责 run 流
  workspace.ts     ← 新增，负责 workspace 请求/响应
```

### 2.1 设计原则

- **共享连接**：与 `chat.ts` 共用同一个 `getLoomWsUrl()` 和 WebSocket 实例
- **请求-响应匹配**：每个请求携带 `id`（UUID），响应中同 `id` 回调 resolve
- **断线重连**：连接断开后自动重连并重发 pending 请求

### 2.2 核心 API

```ts
// src/services/workspace.ts

export interface WorkspaceClient {
  listWorkspaces(): Promise<WorkspaceMeta[]>
  createWorkspace(name?: string): Promise<string>           // 返回 workspace_id
  listThreads(workspaceId: string): Promise<ThreadInWorkspace[]>
  addThread(workspaceId: string, threadId: string): Promise<void>
  removeThread(workspaceId: string, threadId: string): Promise<void>
  dispose(): void
}

export function createWorkspaceClient(options?: {
  wsUrl?: string
}): WorkspaceClient
```

### 2.3 实现策略

**方案 A（推荐）：共享 WebSocket 管道**

创建 `src/services/connection.ts`，将 WebSocket 连接生命周期抽离为单例：

```
connection.ts      ← 管理 WebSocket 实例、消息分发
  ├→ chat.ts       ← 注册 "run" / "run_end" / "error" 等消息处理
  └→ workspace.ts  ← 注册 "workspace_*" 消息处理
```

- `connection.ts` 维护一个 `WebSocket` 实例和 `Map<string, (msg) => void>` 消息路由表
- `chat.ts` 和 `workspace.ts` 各自通过 `send(json)` + `onResponse(id, cb)` 交互
- 这样 `useChat` 和 `useWorkspace` 可以在同一连接上并行工作

**方案 B（简单但冗余）：独立连接**

workspace 独立建立 WebSocket。优点是改动最小，缺点是两个连接浪费资源。

→ 推荐方案 A。可以在 `services/chat.ts` 中现有逻辑的基础上提取共享层。

---

## 3. React Hooks — `src/hooks/useWorkspace.ts`

### 3.1 核心 Hook

```ts
export function useWorkspace() {
  // 状态
  const [workspaces, setWorkspaces] = useState<WorkspaceMeta[]>([])
  const [activeWorkspaceId, setActiveWorkspaceId] = useState<string | null>(null)
  const [threads, setThreads] = useState<ThreadInWorkspace[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // 操作
  const loadWorkspaces: () => Promise<void>
  const createWorkspace: (name?: string) => Promise<string>
  const selectWorkspace: (id: string) => Promise<void>
  const addThreadToWorkspace: (threadId: string) => Promise<void>
  const removeThreadFromWorkspace: (threadId: string) => Promise<void>

  return {
    workspaces, activeWorkspaceId, threads, loading, error,
    loadWorkspaces, createWorkspace, selectWorkspace,
    addThreadToWorkspace, removeThreadFromWorkspace,
  }
}
```

### 3.2 与 useThread 联动

修改 `src/hooks/useThread.ts`：

- 添加 `setThreadId` 外部接口（当前只有 `resetThread`）
- 让 `useWorkspace` 可以在切换 workspace 时将 thread 切换到该 workspace 下的某个 thread
- 在 `useWorkspace.selectWorkspace()` 中：
  1. 调用 `listThreads(workspaceId)` 拿到线程列表
  2. 如果有现存线程，`setThreadId(threads[0].thread_id)` 切换到最新线程
  3. 如果空列表，`resetThread()` 创建新线程并自动 `addThread`

---

## 4. UI 组件

### 4.1 新增组件

```
src/components/workspace/
  WorkspaceSelector.tsx    ← 切换/创建工作空间的下拉菜单
  ThreadList.tsx            ← workspace 下的对话线程列表
  WorkspacePanel.tsx        ← 侧边面板：组合 Selector + ThreadList
  index.ts
```

### 4.2 WorkspaceSelector

- 顶部下拉选择器，显示当前工作空间名称
- "创建工作空间" 按钮弹出行内输入框
- 选中 workspace 后触发 `useWorkspace.selectWorkspace`

### 4.3 ThreadList

- 显示当前 workspace 下的线程列表
- 点击线程 ↔ 切换 `useThread.threadId`
- "新对话" 按钮创建新 thread 并 addThread
- 线程项显示 `created_at_ms` 格式化时间，可选显示最后一条消息摘要

### 4.4 设计风格

沿用现有 Dashboard 的视觉语言：
- 圆角卡片、`bg-muted/40` 背景、`border-border/60` 边框
- 标题使用 `text-xs font-semibold uppercase tracking-widest text-muted-foreground`
- 按钮使用 shadcn/ui 的 `button.tsx` 变体

---

## 5. 页面整合 — `ChatPage-new.tsx`

### 5.1 布局变更

现有布局：

```
┌──────────────┬──────────┬─────────────────┐
│  FileTree     │ Dashboard│  AgentChatSidebar│
│  Sidebar      │          │                  │
└──────────────┴──────────┴─────────────────┘
```

整合后布局：

```
┌─────────┬──────────────┬─────────────────┐
│Workspace│  Dashboard / │  AgentChatSidebar│
│ Panel   │  Chat View   │                  │
│         │              │                  │
│ [Select]│              │                  │
│ [Thread]│              │                  │
│ [List]  │              │                  │
└─────────┴──────────────┴─────────────────┘
```

- 将 `FileTreeSidebar` 替换为 `WorkspacePanel`（或作为可切换的 tab：文件树 / 工作空间）
- `WorkspacePanel` 宽度 240–280px，可折叠

### 5.2 组件结构

```tsx
// ChatPage-new.tsx
function ChatPage() {
  const workspace = useWorkspace()
  const { threadId, setThreadId, resetThread } = useThread()
  const chat = useChat({ threadId, ... })

  // workspace 变更时联动 thread
  useEffect(() => {
    if (workspace.activeWorkspaceId) {
      const threads = workspace.threads
      if (threads.length > 0) {
        setThreadId(threads[0].thread_id)
      } else {
        const newId = resetThread()
        workspace.addThreadToWorkspace(newId)
      }
    }
  }, [workspace.activeWorkspaceId])

  return (
    <ChatErrorBoundary>
      <div className="flex h-screen ...">
        <WorkspacePanel {...workspace} threadId={threadId} onSelectThread={setThreadId} />
        <div className="flex-1 min-w-0">
          <DashboardView ... />
        </div>
        <AgentChatSidebar ... />
      </div>
    </ChatErrorBoundary>
  )
}
```

---

## 6. 数据流总览

```
┌────────────────────────────────┐
│            Web UI              │
│                                │
│  useWorkspace ─── WorkspacePanel│
│  useThread ────── AgentChatSidebar│
│  useChat ──────── MessageList   │
│       │            │            │
│       ▼            ▼            │
│  services/workspace.ts          │
│  services/chat.ts              │
│       │            │            │
│       ▼            ▼            │
│  services/connection.ts         │
│       │ (single WebSocket)      │
└───────┼────────────────────────┘
        │ ws://127.0.0.1:8080
        ▼
┌────────────────────────┐
│     loom serve          │
│  WebSocket handler      │
│  ├─ run → agent dispatch │
│  └─ workspace_* → store │
│       │                  │
│       ▼                  │
│  loom-workspace::Store   │
│  (SQLite)               │
└────────────────────────┘
```

---

## 7. 实施步骤（建议顺序）

| 步骤 | 文件 | 描述 |
|------|------|------|
| **1** | `src/types/protocol/loom.ts` | 添加 workspace 请求/响应类型 |
| **2** | `src/services/connection.ts` | 提取共享 WebSocket 连接管理，从 chat.ts 重构 |
| **3** | `src/services/workspace.ts` | 实现 WorkspaceClient，发送/接收 workspace 请求 |
| **4** | `src/services/chat.ts` | 改为使用 connection.ts 共享层 |
| **5** | `src/hooks/useWorkspace.ts` | 实现 workspace 状态管理 hook |
| **6** | `src/hooks/useThread.ts` | 增加 `setThreadId` 导出 |
| **7** | `src/components/workspace/WorkspaceSelector.tsx` | workspace 下拉选择器 |
| **8** | `src/components/workspace/ThreadList.tsx` | 线程列表组件 |
| **9** | `src/components/workspace/WorkspacePanel.tsx` | 组合面板 |
| **10** | `src/pages/ChatPage-new.tsx` | 整合 WorkspacePanel + 联动逻辑 |
| **11** | 测试 | 单元测试 + manual 集成验证 |

---

## 8. 关键约束和注意事项

- **同源 WebSocket**：workspace 请求和 run 请求走同一条连接，必须在 `connection.ts` 中做好消息路由（按 `type` 前缀分发）
- **请求 id 匹配**：每个请求生成唯一 `id`（`crypto.randomUUID()`），响应中匹配同 `id` 来 resolve Promise
- **错误处理**：所有 workspace 请求可能返回 `type: "error"` 响应，需要在 hook 层妥善处理
- **Run 请求的 workspace_id**：后端 `RunRequest` 已有 `workspace_id` 字段。当 `activeWorkspaceId` 存在时，`chat.ts` 的 `sendMessage` 需要将 `workspace_id` 传入 run 请求，这样 agent 运行时线程会自动注册到 workspace
- **localStorage 持久化**：`activeWorkspaceId` 应持久化到 localStorage，下次打开自动恢复
- **空状态**：首次使用时没有 workspace，需要引导用户创建

---

## 9. Run 请求携带 workspace_id

当前 `chat.ts` 发送 `RunRequest` 时未传 `workspace_id`。整合后需要：

```ts
// chat.ts sendMessage 修改点
const payload = {
  type: 'run',
  id: requestId,
  message: ...,
  agent: ...,
  thread_id: threadId,
  workspace_id: workspaceId || null,  // ← 新增
  working_folder: ...,
}
```

这样后端 `serve` 在处理 Run 时会自动将 thread 注册到对应的 workspace（见 `connection.rs` 中 `ClientRequest::Run` 的处理分支）。