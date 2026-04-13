# 最近会话查询 — 方案设计文档

## 1. 现状分析

### 1.1 目标

用户在 Web 端聊天后，能够查询到历史会话列表，点击可恢复对话上下文。

### 1.2 现有代码能力

| 模块 | 能力 | 状态 |
|------|------|------|
| `loom::UserMessageStore` | 按 thread 存取消息 | ✅ 接口完整 |
| `loom::SqliteUserMessageStore` | SQLite 实现 | ✅ 实现完整 |
| `loom-workspace::Store` | workspace/thread 关联 | ✅ 实现完整 |
| `serve` 消息写入 | Run 时 append 用户+助手消息 | ✅ 逻辑完整 |
| `serve` thread 注册 | Run 时注册 thread 到 workspace | ✅ 逻辑完整 |
| `serve` UserMessageStore 启用 | 需要 `USER_MESSAGE_DB` 环境变量 | ❌ 默认不启用 |
| Web 发送 workspace_id | sendMessage 未传 workspace_id | ❌ 缺失 |
| Web 查询历史消息 | 无 `user_messages` 服务调用 | ❌ 缺失 |
| Web thread 列表展示 | 只显示 thread_id，无标题/摘要 | ❌ 缺失 |

### 1.3 断链点

```
Web 发消息            Serve 处理                持久化              Web 查询
┌──────────┐      ┌──────────────┐       ┌──────────────┐     ┌──────────┐
│ thread_id │─────▶│ thread_id    │       │              │     │          │
│ model     │      │ workspace_id │─✗────▶│  workspace   │─✗──▶│ 会话列表  │
│ agent     │      │              │       │ _threads     │     │          │
│           │      │ user_msg     │       │              │     │          │
│           │      │  _store      │─✗────▶│ user_messages│─✗──▶│ 历史消息  │
└──────────┘      └──────────────┘       └──────────────┘     └──────────┘

✗ = 断链点
```

**三个断链点：**

1. **Serve 默认不启用消息存储** — 不设 `USER_MESSAGE_DB` 则消息不落盘
2. **Web 不传 `workspace_id`** — thread 无法注册到 workspace，无法按 workspace 查询
3. **Web 无查询接口** — 没有调用 `user_messages` 和 `workspace_thread_list` 来恢复会话

---

## 2. 方案设计

### 2.1 整体思路

复用现有 workspace 机制：为每个用户自动创建一个默认 workspace，所有聊天 thread 自动关联到该 workspace。消息通过 `UserMessageStore` 持久化。查询时通过 workspace 获取 thread 列表，再按 thread 加载消息。

### 2.2 数据模型

```
Workspace (默认自动创建)
  │
  ├── Thread A ──── UserMessages [msg1, msg2, msg3, ...]
  ├── Thread B ──── UserMessages [msg1, msg2, ...]
  └── Thread C ──── UserMessages [msg1, ...]
```

**无需新增 Rust 协议类型**，现有协议已够用：

```
请求                          响应
workspace_thread_list    →    threads: [{ thread_id, created_at_ms }]
user_messages            →    messages: [{ role, content }]
workspace_create         →    workspace: { id, name, created_at_ms }
```

### 2.3 改动范围

```
                    改动                    
层            文件                        改动内容
─────────────────────────────────────────────────────────
serve         lib.rs                     UserMessageStore 默认启用，无需环境变量
serve         lib.rs                     WorkspaceStore 默认创建 default workspace

web/service   chat.ts                    sendMessage 传入 workspace_id
web/service   + userMessages.ts (新)     封装 user_messages 查询
web/service   workspace.ts              已有，无需改动

web/hook      useChat.ts                发送时传 workspace_id；新增 loadThreadMessages
web/hook      useThread.ts              新建 thread 时自动关联到默认 workspace

web/page      ChatPage.tsx              会话列表从 threads 构建，点击恢复消息
```

---

## 3. 详细设计

### 3.1 Serve: 默认启用消息存储

**文件**: `serve/src/lib.rs`

**现状**: 需要环境变量 `USER_MESSAGE_DB` 才启用

**改动**: 默认使用 `serve.db` 中的 user_messages 表

```rust
// Before
fn setup_user_message_store() -> Option<Arc<dyn loom::UserMessageStore>> {
    let db_path = std::env::var("USER_MESSAGE_DB")
        .ok()
        .and_then(|path| loom::SqliteUserMessageStore::new(&path).ok())
        .map(|store| Arc::new(store) as Arc<dyn loom::UserMessageStore>);
    ...
}

// After
fn setup_user_message_store() -> Option<Arc<dyn loom::UserMessageStore>> {
    let db_path = std::env::var("USER_MESSAGE_DB")
        .ok()
        .unwrap_or_else(|| "serve.db".to_string());

    match loom::SqliteUserMessageStore::new(&db_path) {
        Ok(store) => {
            info!("✓ User message store initialized (db: {})", db_path);
            Some(Arc::new(store) as Arc<dyn loom::UserMessageStore>)
        }
        Err(e) => {
            warn!("⚠️  Failed to init user message store: {}", e);
            None
        }
    }
}
```

### 3.2 Serve: 启动时创建默认 Workspace

**文件**: `serve/src/lib.rs`

**改动**: 服务启动时，自动创建一个默认 workspace（如不存在）

```rust
fn ensure_default_workspace(store: &loom_workspace::Store) -> String {
    const DEFAULT_WORKSPACE_NAME: &str = "default";

    // 尝试查找已有 workspace
    if let Ok(workspaces) = store.list_workspaces() {
        if let Some(first) = workspaces.first() {
            return first.id.clone();
        }
    }

    // 没有 workspace，创建默认的
    match store.create_workspace(Some(DEFAULT_WORKSPACE_NAME)) {
        Ok(ws) => {
            info!("✓ Created default workspace: {}", ws.id);
            ws.id
        }
        Err(e) => {
            warn!("⚠️  Failed to create default workspace: {}", e);
            String::new()
        }
    }
}
```

AppState 中保存 `default_workspace_id`，通过 WebSocket 响应告知前端。

### 3.3 Web: sendMessage 传入 workspace_id

**文件**: `web/src/services/chat.ts`

```typescript
// Before
const payload = {
    type: 'run',
    message: content,
    agent: agentValue,
    thread_id: options.threadId,
    model: options.model,
    // workspace_id 缺失
}

// After
type SendMessageOptions = {
    threadId?: string
    workspaceId?: string    // 新增
    agent?: string
    model?: string
    sessionId?: string
    onChunk?: (chunk: string) => void
    onEvent?: (event: LoomStreamEvent) => void
}

const payload = {
    type: 'run',
    message: content,
    agent: agentValue,
    thread_id: options.threadId,
    workspace_id: options.workspaceId,  // 新增
    working_folder: workingFolder,
    model: options.model,
}
```

### 3.4 Web: 新增 userMessages 查询服务

**文件**: `web/src/services/userMessages.ts` (新文件)

```typescript
import type { LoomServerMessage } from '../types/protocol/loom'
import { getConnection } from './connection'

export type UserMessageItem = {
    role: string
    content: string
}

export type UserMessagesResponse = {
    type: 'user_messages'
    id: string
    thread_id: string
    messages: UserMessageItem[]
    has_more: boolean | null
}

/**
 * 查询指定 thread 的历史消息
 */
export async function getUserMessages(
    threadId: string,
    options?: { before?: number; limit?: number }
): Promise<UserMessageItem[]> {
    const resp = await getConnection().request({
        type: 'user_messages',
        id: crypto.randomUUID(),
        thread_id: threadId,
        before: options?.before,
        limit: options?.limit,
    })

    const msg = resp as UserMessagesResponse
    return msg.messages ?? []
}
```

### 3.5 Web: useChat 接入 workspace_id

**文件**: `web/src/hooks/useChat.ts`

```typescript
export function useChat(options?: {
    threadId?: string
    workspaceId?: string    // 新增
    agentId?: string
    model?: string
}) {
    const workspaceId = options?.workspaceId

    const sendMessage = useCallback(async (text: string) => {
        // ...
        const reply = await sendChatMessage(text, {
            threadId,
            workspaceId,     // 传入 workspace_id
            agent: agentId,
            model,
            onChunk: handleTextChunk,
            onEvent: handleEvent,
        })
        // ...
    }, [threadId, workspaceId, agentId, model, ...])
}
```

### 3.6 Web: 会话列表数据增强

**文件**: `web/src/pages/ChatPage.tsx`

**现状**: threads 只有 `thread_id` 和 `created_at_ms`

**改动**: 
- 从 `user_messages` 加载每个 thread 的第一条用户消息作为标题/摘要
- 展示消息数量

```typescript
// 加载会话摘要
async function loadSessionSummary(
    threads: ThreadInWorkspace[]
): Promise<Session[]> {
    return Promise.all(threads.map(async (t) => {
        const messages = await getUserMessages(t.thread_id, { limit: 1 })
        const firstMsg = messages.find(m => m.role === 'user')

        return {
            id: t.thread_id,
            title: firstMsg?.content?.slice(0, 50) || t.thread_id.slice(0, 8),
            createdAt: new Date(t.created_at_ms).toISOString(),
            updatedAt: new Date(t.created_at_ms).toISOString(),
            lastMessage: firstMsg?.content?.slice(0, 100) || '',
            messageCount: messages.length,
            agent: '',
            model: '',
            isPinned: false,
        }
    }))
}
```

### 3.7 Web: 点击会话恢复消息

**文件**: `web/src/hooks/useChat.ts`

**新增方法**: `loadHistory`

```typescript
const loadHistory = useCallback(async (targetThreadId: string) => {
    const messages = await getUserMessages(targetThreadId)
    
    const uiMessages: UIMessageItemProps[] = []
    
    for (const msg of messages) {
        uiMessages.push({
            id: crypto.randomUUID(),
            sender: msg.role === 'user' ? 'user' : 'assistant',
            timestamp: new Date().toISOString(),
            content: [{
                type: 'text',
                text: msg.content,
                format: 'plain',
            }],
        })
    }
    
    setMessages(uiMessages)
}, [])
```

---

## 4. 数据流（改动后）

```
Web 发消息                Serve 处理               持久化              Web 查询
┌───────────────┐     ┌───────────────┐     ┌───────────────┐    ┌───────────┐
│ thread_id     │     │               │     │               │    │           │
│ workspace_id  │────▶│ thread_id     │     │               │    │           │
│ model         │     │ workspace_id  │────▶│ workspace     │───▶│ 会话列表   │
│ agent         │     │ model         │     │ _threads      │    │ (threads) │
│               │     │               │     │               │    │           │
│               │     │ user_msg_store│────▶│ user_messages │───▶│ 历史消息   │
│               │     │ (默认启用)     │     │ (SQLite)      │    │ (messages)│
└───────────────┘     └───────────────┘     └───────────────┘    └───────────┘
```

---

## 5. 实现步骤

### Phase 1: 后端 — 打通存储（预计改动 2 个文件）

| 步骤 | 文件 | 改动 |
|------|------|------|
| 1 | `serve/src/lib.rs` | `setup_user_message_store` 默认启用 |
| 2 | `serve/src/lib.rs` | 启动时 `ensure_default_workspace` |

### Phase 2: 前端 — 打通请求（预计改动 3 个文件，新增 1 个）

| 步骤 | 文件 | 改动 |
|------|------|------|
| 3 | `web/src/services/chat.ts` | `SendMessageOptions` 增加 `workspaceId`，payload 传入 |
| 4 | `web/src/services/userMessages.ts` | **新建**，封装 `getUserMessages` |
| 5 | `web/src/hooks/useChat.ts` | 传入 `workspaceId`，新增 `loadHistory` |
| 6 | `web/src/hooks/useThread.ts` | 新建 thread 后自动调用 `addThread` 关联到默认 workspace |

### Phase 3: 前端 — UI 展示（预计改动 2 个文件）

| 步骤 | 文件 | 改动 |
|------|------|------|
| 7 | `web/src/pages/ChatPage.tsx` | 会话列表加载摘要，点击恢复 |
| 8 | `web/src/components/sessions/SessionList.tsx` | 接入真实数据替换 mock |

---

## 6. 不改动的部分

- **Rust 协议层** — 现有 `ClientRequest` / `ServerResponse` 已覆盖所需类型
- **loom-workspace** — `Store` 接口已完整
- **loom::UserMessageStore** — trait 和 SQLite 实现已完整
- **serve 请求分发** — `WorkspaceThreadList`、`UserMessages` handler 已存在
- **web workspace 服务** — `listThreads`、`addThread` 已实现

---

## 7. 验证方式

1. 启动 serve，确认日志输出：
   ```
   ✓ User message store initialized (db: serve.db)
   ✓ Created default workspace: xxx
   ```

2. Web 端发送消息，确认 serve 日志输出：
   ```
   🤖 Model requested: anthropic/claude-3-5-sonnet-20241022
   📂 Registering thread xxx to workspace yyy
   💬 Appending user message to thread xxx
   💬 Appending assistant message to thread xxx
   ```

3. 刷新页面或切换 thread 后，点击会话列表中的历史会话，确认消息恢复显示

4. SQLite 中确认数据落盘：
   ```sql
   SELECT * FROM workspace_threads;
   SELECT * FROM user_messages ORDER BY seq;
   ```
