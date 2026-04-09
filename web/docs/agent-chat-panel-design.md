# Agent 对话面板功能 - 设计与开发方案

## 概述

在现有 Dashboard 中集成可收起的 Agent 对话面板，支持用户与助手代理进行实时对话。

---

## 一、交互方案

### 1.1 布局设计

#### 展开状态
- 左侧：FileTree 文件树
- 中间：Dashboard (Agent Grid + Activity Feed)  
- 右侧：Chat Panel (Agent选择 + Messages + Composer)

#### 收起状态
- 左侧：FileTree
- 中间：Dashboard 扩展
- 右侧：极简图标 [💬 3] 显示未读数

### 1.2 收起交互方式

**方式 A：按钮切换（推荐）**
- 展开时：面板右上角 [◀ 收起]
- 收起时：右侧竖条 [▶ 展开]

**方式 B：拖拽调整**
- 拖拽手柄调整宽度
- 最小宽度 320px
- <200px 自动收起

### 1.3 状态记忆

```typescript
interface ChatPanelState {
  collapsed: boolean
  width: number
  selectedAgentId: string | null
}
```

存储：LocalStorage (`chatPanelState`)

### 1.4 动画

- 时长：250ms
- 缓动：ease-in-out
- 宽度范围：320px - 600px

### 1.5 响应式

| 设备 | 行为 |
|------|------|
| Desktop (>1024px) | 面板 400px，可拖拽 |
| Tablet (768-1024px) | 面板 320px |
| Mobile (<768px) | 底部抽屉/全屏 Modal |

---

## 二、技术方案

### 2.1 组件架构

```
ChatPage-new.tsx
├── FileTreeSidebar
├── DashboardView
└── AgentChatSidebar (新增)
    ├── AgentSelector
    ├── MessageList
    └── MessageComposer
```

### 2.2 新增组件

**AgentChatSidebar**
```typescript
interface AgentChatSidebarProps {
  collapsed: boolean
  onToggle: () => void
  width: number
  onResize: (width: number) => void
}
```

**AgentSelector**
```typescript
interface AgentSelectorProps {
  agents: AgentInfo[]
  selectedAgentId: string | null
  onSelect: (agentId: string) => void
}
```

**CollapsedPanel**
```typescript
interface CollapsedPanelProps {
  unreadCount: number
  onExpand: () => void
}
```

**ResizeHandle**
```typescript
interface ResizeHandleProps {
  onResize: (delta: number) => void
  onResizeEnd: () => void
}
```

### 2.3 Hooks

**useChatPanel**
```typescript
interface UseChatPanelReturn {
  collapsed: boolean
  width: number
  selectedAgentId: string | null
  toggle: () => void
  expand: () => void
  collapse: () => void
  setWidth: (width: number) => void
  selectAgent: (agentId: string) => void
}
```

**useAgentChat**
```typescript
interface UseAgentChatReturn {
  messages: UIMessageItemProps[]
  isStreaming: boolean
  error: string | null
  unreadCount: number
  sendMessage: (text: string) => Promise<void>
  markAsRead: () => void
}
```

---

## 三、实现步骤

### Phase 1：基础框架（2-3天）
- [ ] 创建 AgentChatSidebar 组件骨架
- [ ] 实现 useChatPanel hook
- [ ] 添加收起/展开逻辑
- [ ] 集成到 ChatPage

### Phase 2：Agent 选择（2天）
- [ ] 创建 AgentSelector 组件
- [ ] 实现 useAgentChat hook
- [ ] 修改 useChat 支持 agentId
- [ ] 实现 Agent 切换

### Phase 3：拖拽调整（1-2天）
- [ ] 创建 ResizeHandle 组件
- [ ] 实现拖拽逻辑
- [ ] 添加宽度限制

### Phase 4：状态持久化（1天）
- [ ] LocalStorage 存储
- [ ] 状态恢复逻辑
- [ ] 默认值处理

### Phase 5：响应式优化（1-2天）
- [ ] 响应式断点
- [ ] 移动端适配
- [ ] 动画优化
- [ ] 性能优化

### Phase 6：测试文档（1-2天）
- [ ] 单元测试
- [ ] 组件测试
- [ ] Storybook stories
- [ ] 文档更新

---

## 四、技术细节

### 4.1 CSS 动画
```css
.chat-sidebar {
  transition: width 250ms ease-in-out;
  overflow: hidden;
}
.chat-sidebar--collapsed { width: 48px; }
.chat-sidebar--expanded { width: var(--chat-panel-width, 400px); }

.resize-handle {
  width: 4px;
  cursor: col-resize;
  transition: background-color 150ms;
}
.resize-handle:hover { background: var(--color-primary); }
```

### 4.2 性能优化
1. 虚拟列表 - 长消息列表
2. 防抖 - 拖拽使用 requestAnimationFrame
3. 懒加载 - 收起时跳过渲染

### 4.3 无障碍
```html
<div role="complementary" aria-label="Agent Chat Panel" aria-expanded={!collapsed}>
  <button aria-label={collapsed ? '展开' : '收起'}>
    {collapsed ? '▶' : '◀'}
  </button>
</div>
```

---

## 五、时间估算

| 阶段 | 时间 |
|------|------|
| Phase 1 基础框架 | 2-3 天 |
| Phase 2 Agent 选择 | 2 天 |
| Phase 3 拖拽调整 | 1-2 天 |
| Phase 4 状态持久化 | 1 天 |
| Phase 5 响应式优化 | 1-2 天 |
| Phase 6 测试文档 | 1-2 天 |
| **总计** | **8-12 天** |

---

## 六、风险应对

| 风险 | 影响 | 措施 |
|------|------|------|
| WebSocket 稳定性 | 中 | 重连机制、离线提示 |
| 大消息性能 | 低 | 虚拟列表、分页 |
| 移动端体验 | 中 | 独立移动端设计 |
| 状态丢失 | 低 | LocalStorage 持久化 |

---

## 七、文件清单

### 新增
```
src/components/chat/AgentChatSidebar.tsx
src/components/chat/CollapsedPanel.tsx
src/components/chat/AgentSelector.tsx
src/components/chat/ResizeHandle.tsx
src/hooks/useChatPanel.ts
src/hooks/useAgentChat.ts
src/__tests__/hooks/useChatPanel.test.ts
src/__tests__/components/AgentChatSidebar.test.tsx
```

### 修改
```
src/pages/ChatPage-new.tsx
src/hooks/useChat.ts
src/types/chat.ts
src/components/chat/index.ts
src/hooks/index.ts
```

---

**版本：** 1.0  
**日期：** 2025-08-19
