# Web 前端重构说明

## 架构概览

本次重构采用了**组件属性与协议分离**的架构设计，使组件更加通用和可复用。

### 三层架构

```
┌─────────────────────────────────────────┐
│         协议层 (Protocol Layer)          │
│  - LoomWebSocketMessage                 │
│  - LoomStreamEvent                      │
│  - 协议特定的数据结构                     │
└─────────────────────────────────────────┘
                   ↓
            适配器层 (Adapter Layer)
                   ↓
┌─────────────────────────────────────────┐
│         组件层 (Component Layer)         │
│  - MessageItemProps                     │
│  - ToolBlockProps                       │
│  - 协议无关的通用类型                      │
└─────────────────────────────────────────┘
```

## 目录结构

```
web/src/
├── adapters/            # 适配器层
│   ├── MessageAdapter.ts
│   ├── ToolBlockAdapter.ts
│   └── index.ts
├── components/          # UI 组件
│   ├── chat/           # 聊天相关组件
│   │   ├── MessageItem.tsx
│   │   ├── MessageList.tsx
│   │   ├── MessageBlockView.tsx
│   │   ├── TextMessage.tsx
│   │   └── ToolMessage.tsx
│   ├── common/         # 通用组件
│   │   └── ConnectionStatus.tsx
│   ├── error/          # 错误处理
│   │   └── ErrorBoundary.tsx
│   └── layout/         # 布局组件
│       └── ChatLayout.tsx
├── hooks/              # 自定义 Hooks
│   ├── useWebSocket.ts
│   ├── useThread.ts
│   ├── useMessages.ts
│   ├── useChat.ts
│   └── index.ts
├── types/              # 类型定义
│   ├── protocol/       # 协议类型
│   │   ├── loom.ts
│   │   └── index.ts
│   └── ui/             # UI 类型
│       ├── message.ts
│       └── index.ts
├── utils/              # 工具函数
│   ├── format.ts
│   └── index.ts
├── pages/              # 页面组件
│   └── ChatPage.tsx
├── App.tsx
└── main.tsx
```

## 核心设计原则

### 1. 协议无关的组件

组件只依赖通用的 UI 类型，不依赖任何特定的协议：

```typescript
// ❌ 旧方式 - 组件依赖协议
function MessageItem({ event }: { event: LoomStreamEvent }) {
  // 组件与协议耦合
}

// ✅ 新方式 - 组件使用通用类型
function MessageItem({ sender, timestamp, content }: MessageItemProps) {
  // 组件协议无关
}
```

### 2. 适配器模式

通过适配器将协议数据转换为组件属性：

```typescript
// 协议数据
const loomEvent: LoomStreamEvent = {
  type: 'assistant_text',
  id: '1',
  text: 'Hello',
  createdAt: '2024-01-01T00:00:00Z'
}

// 通过适配器转换
const uiProps = MessageAdapter.toUI(loomEvent)

// 组件使用
<MessageItem {...uiProps} />
```

### 3. 自定义 Hooks

将业务逻辑封装在自定义 Hooks 中：

- `useWebSocket` - WebSocket 连接管理
- `useThread` - 线程状态管理
- `useMessages` - 消息状态管理
- `useChat` - 聊天功能协调

### 4. 错误边界

使用 Error Boundary 捕获和处理错误：

```typescript
<ErrorBoundary>
  <ChatPage />
</ErrorBoundary>
```

## 使用示例

### 基本使用

```typescript
import { useChat } from './hooks'
import { ChatLayout } from './components/layout/ChatLayout'

function ChatPage() {
  const {
    messages,
    isStreaming,
    connectionStatus,
    error,
    sendMessage,
    resetThread,
  } = useChat()

  return (
    <ChatLayout>
      <ConnectionStatus status={connectionStatus} />
      <MessageList messages={messages} />
      <MessageComposer
        disabled={isStreaming}
        onSend={sendMessage}
      />
    </ChatLayout>
  )
}
```

### 使用适配器

```typescript
import { MessageAdapter } from './adapters'
import type { LoomStreamEvent } from './types/protocol/loom'

// 转换单个消息
const event: LoomStreamEvent = { /* ... */ }
const messageProps = MessageAdapter.toUI(event)

// 转换消息列表
const events: LoomStreamEvent[] = [/* ... */]
const messages = MessageAdapter.toUIList(events)
```

### 自定义组件

```typescript
import { MessageItem } from './components/chat/MessageItem'
import type { MessageItemProps } from './types/ui/message'

// 组件只依赖通用类型
const props: MessageItemProps = {
  id: '1',
  sender: 'user',
  timestamp: '2024-01-01T00:00:00Z',
  content: [
    { type: 'text', text: 'Hello' }
  ]
}

<MessageItem {...props} />
```

## 优势

### 1. 协议无关
- 组件可以在任何项目中复用
- 轻松切换不同的后端协议
- 降低耦合度

### 2. 类型安全
- 完整的 TypeScript 类型定义
- 编译时类型检查
- 更好的 IDE 支持

### 3. 易于测试
- 组件测试不需要模拟协议
- 适配器可以独立测试
- Hooks 可以独立测试

### 4. 可维护性
- 清晰的代码结构
- 职责分离
- 易于理解和修改

### 5. 性能优化
- 组件使用 React.memo
- 使用 useMemo 和 useCallback
- 按需渲染

## 迁移指南

### 从旧代码迁移

1. **替换导入**
```typescript
// 旧代码
import type { Message } from './types/chat'

// 新代码
import type { MessageItemProps } from './types/ui/message'
```

2. **使用适配器**
```typescript
// 旧代码
const message: Message = { /* ... */ }

// 新代码
const event: LoomStreamEvent = { /* ... */ }
const message = MessageAdapter.toUI(event)
```

3. **使用新 Hooks**
```typescript
// 旧代码
const [messages, setMessages] = useState([])

// 新代码
const { messages, addMessage } = useMessages()
```

## 下一步计划

1. ✅ 基础架构重构
2. ⬜ 单元测试
3. ⬜ 性能优化
4. ⬜ 可访问性增强
5. ⬜ 文档完善

## 相关文档

- [Web 前端重构方案](../../docs/idea/web-refactoring-plan-zh.md)
- [组件属性与协议分离方案](../../docs/idea/web-component-protocol-separation-plan.md)
