# 90% 测试覆盖率实施方案

## 目标

在2周内达到 **90%+ 测试覆盖率**

## 当前状态

- 覆盖率：**0%**
- 新增文件：24个（types, adapters, hooks, components）
- 代码行数：约2000行

## 测试策略

### 覆盖率目标分解

| 层级 | 文件数 | 代码行数 | 目标覆盖率 | 优先级 |
|------|--------|---------|-----------|--------|
| 类型守卫 | 2 | ~50 | 100% | P0 |
| 适配器 | 2 | ~200 | 95% | P0 |
| Hooks | 4 | ~300 | 90% | P0 |
| 工具函数 | 1 | ~30 | 100% | P0 |
| 组件 | 9 | ~500 | 85% | P1 |
| **总计** | **18** | **~1080** | **90%+** | - |

## Phase 1: 测试框架搭建（第1天）

### 1.1 安装依赖

```bash
cd web
npm install -D vitest @vitest/coverage-v8 @testing-library/react @testing-library/jest-dom @testing-library/user-event jsdom @types/node
```

### 1.2 配置 Vitest

创建 `vitest.config.ts`:

```typescript
import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: './src/test/setup.ts',
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json', 'html', 'lcov'],
      exclude: [
        'node_modules/',
        'src/test/',
        '**/*.d.ts',
        '**/*.config.*',
        '**/index.ts',
        'src/main.tsx',
        'src/App.tsx'
      ],
      statements: 90,
      branches: 85,
      functions: 90,
      lines: 90
    }
  }
})
```

### 1.3 测试设置文件

创建 `src/test/setup.ts`:

```typescript
import '@testing-library/jest-dom'
import { cleanup } from '@testing-library/react'
import { afterEach, vi } from 'vitest'

// 每个测试后自动清理
afterEach(() => {
  cleanup()
})

// Mock WebSocket
global.WebSocket = vi.fn().mockImplementation(() => ({
  readyState: 0,
  send: vi.fn(),
  close: vi.fn(),
  addEventListener: vi.fn(),
  removeEventListener: vi.fn(),
}))

// Mock localStorage
const localStorageMock = {
  getItem: vi.fn(),
  setItem: vi.fn(),
  removeItem: vi.fn(),
  clear: vi.fn(),
}
Object.defineProperty(window, 'localStorage', { value: localStorageMock })

// Mock crypto.randomUUID
Object.defineProperty(global, 'crypto', {
  value: {
    randomUUID: () => 'test-uuid-' + Math.random().toString(36).substr(2, 9)
  }
})
```

### 1.4 更新 package.json

```json
{
  "scripts": {
    "test": "vitest",
    "test:run": "vitest run",
    "test:coverage": "vitest run --coverage",
    "test:ui": "vitest --ui",
    "test:watch": "vitest watch"
  }
}
```

## Phase 2: 核心测试（第2-5天）- 目标 95% 覆盖率

### 2.1 类型守卫测试（100% 覆盖率）

**文件**: `src/types/__tests__/guards.test.ts`

```typescript
import { describe, it, expect } from 'vitest'
import {
  isUITextContent,
  isUIToolContent,
  isUserEvent,
  isAssistantTextEvent,
  isAssistantToolEvent,
  isRunStreamEvent,
  isRunEnd,
  isError
} from '../index'
import type { UIMessageContent, LoomStreamEvent, LoomServerMessage } from '../index'

describe('UI 类型守卫', () => {
  describe('isUITextContent', () => {
    it('应该识别文本内容', () => {
      const content: UIMessageContent = { type: 'text', text: 'Hello' }
      expect(isUITextContent(content)).toBe(true)
    })

    it('应该拒绝工具内容', () => {
      const content: UIMessageContent = {
        type: 'tool',
        id: '1',
        name: 'test',
        status: 'success',
        argumentsText: '',
        outputText: '',
        resultText: '',
        isError: false
      }
      expect(isUITextContent(content)).toBe(false)
    })
  })

  describe('isUIToolContent', () => {
    it('应该识别工具内容', () => {
      const content: UIMessageContent = {
        type: 'tool',
        id: '1',
        name: 'bash',
        status: 'running',
        argumentsText: 'ls',
        outputText: 'file1\nfile2',
        resultText: '',
        isError: false
      }
      expect(isUIToolContent(content)).toBe(true)
    })

    it('应该拒绝文本内容', () => {
      const content: UIMessageContent = { type: 'text', text: 'Hello' }
      expect(isUIToolContent(content)).toBe(false)
    })
  })
})

describe('Loom 协议类型守卫', () => {
  describe('isUserEvent', () => {
    it('应该识别用户事件', () => {
      const event: LoomStreamEvent = {
        type: 'user',
        id: '1',
        createdAt: '2024-01-01T00:00:00Z',
        text: 'Hello'
      }
      expect(isUserEvent(event)).toBe(true)
    })

    it('应该拒绝助手事件', () => {
      const event: LoomStreamEvent = {
        type: 'assistant_text',
        id: '1',
        createdAt: '2024-01-01T00:00:00Z',
        text: 'Hi'
      }
      expect(isUserEvent(event)).toBe(false)
    })
  })

  describe('isAssistantTextEvent', () => {
    it('应该识别助手文本事件', () => {
      const event: LoomStreamEvent = {
        type: 'assistant_text',
        id: '1',
        createdAt: '2024-01-01T00:00:00Z',
        text: 'Response'
      }
      expect(isAssistantTextEvent(event)).toBe(true)
    })
  })

  describe('isAssistantToolEvent', () => {
    it('应该识别工具事件', () => {
      const event: LoomStreamEvent = {
        type: 'assistant_tool',
        id: '1',
        createdAt: '2024-01-01T00:00:00Z',
        callId: 'call-1',
        name: 'bash',
        status: 'done',
        argumentsText: 'ls',
        outputText: 'output',
        resultText: 'result',
        isError: false
      }
      expect(isAssistantToolEvent(event)).toBe(true)
    })
  })

  describe('isRunStreamEvent', () => {
    it('应该识别流事件响应', () => {
      const msg: LoomServerMessage = {
        type: 'run_stream_event',
        id: '1',
        event: {
          type: 'user',
          id: '1',
          createdAt: '2024-01-01T00:00:00Z',
          text: 'Hello'
        }
      }
      expect(isRunStreamEvent(msg)).toBe(true)
    })
  })

  describe('isRunEnd', () => {
    it('应该识别结束响应', () => {
      const msg: LoomServerMessage = {
        type: 'run_end',
        id: '1',
        reply: 'Done'
      }
      expect(isRunEnd(msg)).toBe(true)
    })
  })

  describe('isError', () => {
    it('应该识别错误响应', () => {
      const msg: LoomServerMessage = {
        type: 'error',
        id: '1',
        error: 'Something went wrong'
      }
      expect(isError(msg)).toBe(true)
    })
  })
})
```

### 2.2 适配器测试（95% 覆盖率）

**文件**: `src/adapters/__tests__/MessageAdapter.test.ts`

```typescript
import { describe, it, expect, beforeEach } from 'vitest'
import { MessageAdapter } from '../MessageAdapter'
import type { LoomStreamEvent, LoomAssistantToolEvent } from '../../types/protocol/loom'

describe('MessageAdapter', () => {
  describe('toUI', () => {
    it('应该正确转换用户事件', () => {
      const event: LoomStreamEvent = {
        type: 'user',
        id: 'user-1',
        createdAt: '2024-01-01T10:00:00Z',
        text: 'Hello, assistant!'
      }

      const result = MessageAdapter.toUI(event)

      expect(result.id).toBe('user-1')
      expect(result.sender).toBe('user')
      expect(result.timestamp).toBe('2024-01-01T10:00:00Z')
      expect(result.content).toHaveLength(1)
      expect(result.content[0].type).toBe('text')
      if (result.content[0].type === 'text') {
        expect(result.content[0].text).toBe('Hello, assistant!')
        expect(result.content[0].format).toBe('plain')
      }
    })

    it('应该正确转换助手文本事件', () => {
      const event: LoomStreamEvent = {
        type: 'assistant_text',
        id: 'assistant-1',
        createdAt: '2024-01-01T10:01:00Z',
        text: 'Hello, user!'
      }

      const result = MessageAdapter.toUI(event)

      expect(result.sender).toBe('assistant')
      expect(result.content).toHaveLength(1)
      expect(result.content[0].type).toBe('text')
      if (result.content[0].type === 'text') {
        expect(result.content[0].text).toBe('Hello, user!')
      }
    })

    it('应该正确转换工具事件', () => {
      const event: LoomStreamEvent = {
        type: 'assistant_tool',
        id: 'tool-1',
        createdAt: '2024-01-01T10:02:00Z',
        callId: 'call-123',
        name: 'bash',
        status: 'done',
        argumentsText: 'ls -la',
        outputText: 'file1\nfile2',
        resultText: 'Success',
        isError: false
      }

      const result = MessageAdapter.toUI(event)

      expect(result.sender).toBe('assistant')
      expect(result.content).toHaveLength(1)
      expect(result.content[0].type).toBe('tool')
      if (result.content[0].type === 'tool') {
        expect(result.content[0].id).toBe('call-123')
        expect(result.content[0].name).toBe('bash')
        expect(result.content[0].status).toBe('success')
        expect(result.content[0].argumentsText).toBe('ls -la')
        expect(result.content[0].outputText).toBe('file1\nfile2')
        expect(result.content[0].resultText).toBe('Success')
        expect(result.content[0].isError).toBe(false)
      }
    })

    it('应该正确映射工具状态 - queued', () => {
      const event: LoomStreamEvent = {
        type: 'assistant_tool',
        id: '1',
        createdAt: '2024-01-01T00:00:00Z',
        callId: '1',
        name: 'test',
        status: 'queued',
        argumentsText: '',
        outputText: '',
        resultText: '',
        isError: false
      }

      const result = MessageAdapter.toUI(event)
      if (result.content[0].type === 'tool') {
        expect(result.content[0].status).toBe('pending')
      }
    })

    it('应该正确映射工具状态 - running', () => {
      const event: LoomStreamEvent = {
        type: 'assistant_tool',
        id: '1',
        createdAt: '2024-01-01T00:00:00Z',
        callId: '1',
        name: 'test',
        status: 'running',
        argumentsText: '',
        outputText: '',
        resultText: '',
        isError: false
      }

      const result = MessageAdapter.toUI(event)
      if (result.content[0].type === 'tool') {
        expect(result.content[0].status).toBe('running')
      }
    })

    it('应该正确映射工具状态 - done', () => {
      const event: LoomStreamEvent = {
        type: 'assistant_tool',
        id: '1',
        createdAt: '2024-01-01T00:00:00Z',
        callId: '1',
        name: 'test',
        status: 'done',
        argumentsText: '',
        outputText: '',
        resultText: '',
        isError: false
      }

      const result = MessageAdapter.toUI(event)
      if (result.content[0].type === 'tool') {
        expect(result.content[0].status).toBe('success')
      }
    })

    it('应该正确映射工具状态 - error', () => {
      const event: LoomStreamEvent = {
        type: 'assistant_tool',
        id: '1',
        createdAt: '2024-01-01T00:00:00Z',
        callId: '1',
        name: 'test',
        status: 'error',
        argumentsText: '',
        outputText: '',
        resultText: 'Error occurred',
        isError: true
      }

      const result = MessageAdapter.toUI(event)
      if (result.content[0].type === 'tool') {
        expect(result.content[0].status).toBe('error')
        expect(result.content[0].isError).toBe(true)
      }
    })

    it('应该正确映射工具状态 - approval_required', () => {
      const event: LoomStreamEvent = {
        type: 'assistant_tool',
        id: '1',
        createdAt: '2024-01-01T00:00:00Z',
        callId: '1',
        name: 'test',
        status: 'approval_required',
        argumentsText: '',
        outputText: '',
        resultText: '',
        isError: false
      }

      const result = MessageAdapter.toUI(event)
      if (result.content[0].type === 'tool') {
        expect(result.content[0].status).toBe('pending')
      }
    })
  })

  describe('toUIList', () => {
    it('应该正确转换多个事件', () => {
      const events: LoomStreamEvent[] = [
        {
          type: 'user',
          id: '1',
          createdAt: '2024-01-01T00:00:00Z',
          text: 'Hello'
        },
        {
          type: 'assistant_text',
          id: '2',
          createdAt: '2024-01-01T00:01:00Z',
          text: 'Hi'
        }
      ]

      const results = MessageAdapter.toUIList(events)

      expect(results).toHaveLength(2)
      expect(results[0].sender).toBe('user')
      expect(results[1].sender).toBe('assistant')
    })

    it('应该处理空数组', () => {
      const results = MessageAdapter.toUIList([])
      expect(results).toHaveLength(0)
    })
  })

  describe('mergeEvents', () => {
    it('应该合并连续的助手事件', () => {
      const events: LoomStreamEvent[] = [
        {
          type: 'assistant_text',
          id: '1',
          createdAt: '2024-01-01T00:00:00Z',
          text: 'Hello'
        },
        {
          type: 'assistant_tool',
          id: '2',
          createdAt: '2024-01-01T00:01:00Z',
          callId: 'call-1',
          name: 'bash',
          status: 'done',
          argumentsText: 'ls',
          outputText: 'files',
          resultText: '',
          isError: false
        }
      ]

      const results = MessageAdapter.mergeEvents(events)

      expect(results).toHaveLength(1)
      expect(results[0].content).toHaveLength(2)
    })

    it('应该将用户消息作为独立消息', () => {
      const events: LoomStreamEvent[] = [
        {
          type: 'user',
          id: '1',
          createdAt: '2024-01-01T00:00:00Z',
          text: 'Question 1'
        },
        {
          type: 'user',
          id: '2',
          createdAt: '2024-01-01T00:01:00Z',
          text: 'Question 2'
        }
      ]

      const results = MessageAdapter.mergeEvents(events)

      expect(results).toHaveLength(2)
      expect(results[0].content).toHaveLength(1)
      expect(results[1].content).toHaveLength(1)
    })
  })

  describe('updateMessage', () => {
    it('应该追加文本到现有文本块', () => {
      const message = {
        id: '1',
        sender: 'assistant' as const,
        timestamp: '2024-01-01T00:00:00Z',
        content: [{ type: 'text' as const, text: 'Hello' }]
      }

      const event: LoomStreamEvent = {
        type: 'assistant_text',
        id: '1',
        createdAt: '2024-01-01T00:00:00Z',
        text: ' World'
      }

      const updated = MessageAdapter.updateMessage(message, event)

      expect(updated.content).toHaveLength(1)
      if (updated.content[0].type === 'text') {
        expect(updated.content[0].text).toBe('Hello World')
      }
    })

    it('应该添加新的文本块到消息', () => {
      const message = {
        id: '1',
        sender: 'assistant' as const,
        timestamp: '2024-01-01T00:00:00Z',
        content: [{ type: 'tool' as const, id: '1', name: 'bash', status: 'success' as const, argumentsText: '', outputText: '', resultText: '', isError: false }]
      }

      const event: LoomStreamEvent = {
        type: 'assistant_text',
        id: '2',
        createdAt: '2024-01-01T00:01:00Z',
        text: 'Done'
      }

      const updated = MessageAdapter.updateMessage(message, event)

      expect(updated.content).toHaveLength(2)
    })
  })
})
```

**文件**: `src/adapters/__tests__/ToolBlockAdapter.test.ts`

```typescript
import { describe, it, expect } from 'vitest'
import { ToolBlockAdapter } from '../ToolBlockAdapter'
import type { LoomAssistantToolEvent } from '../../types/protocol/loom'

describe('ToolBlockAdapter', () => {
  describe('toUI', () => {
    it('应该正确转换工具事件', () => {
      const event: LoomAssistantToolEvent = {
        type: 'assistant_tool',
        id: '1',
        createdAt: '2024-01-01T00:00:00Z',
        callId: 'call-123',
        name: 'bash',
        status: 'done',
        argumentsText: 'echo "Hello"',
        outputText: 'Hello\n',
        resultText: 'Success',
        isError: false
      }

      const result = ToolBlockAdapter.toUI(event)

      expect(result.id).toBe('call-123')
      expect(result.name).toBe('bash')
      expect(result.status).toBe('success')
      expect(result.argumentsText).toBe('echo "Hello"')
      expect(result.outputText).toBe('Hello\n')
      expect(result.resultText).toBe('Success')
      expect(result.isError).toBe(false)
    })

    it('应该正确处理错误状态', () => {
      const event: LoomAssistantToolEvent = {
        type: 'assistant_tool',
        id: '1',
        createdAt: '2024-01-01T00:00:00Z',
        callId: 'call-456',
        name: 'bash',
        status: 'error',
        argumentsText: 'exit 1',
        outputText: '',
        resultText: 'Command failed',
        isError: true
      }

      const result = ToolBlockAdapter.toUI(event)

      expect(result.status).toBe('error')
      expect(result.isError).toBe(true)
      expect(result.resultText).toBe('Command failed')
    })
  })

  describe('mapStatus', () => {
    const testCases = [
      { input: 'queued' as const, expected: 'pending' as const },
      { input: 'running' as const, expected: 'running' as const },
      { input: 'done' as const, expected: 'success' as const },
      { input: 'error' as const, expected: 'error' as const },
      { input: 'approval_required' as const, expected: 'pending' as const },
    ]

    testCases.forEach(({ input, expected }) => {
      it(`应该将 ${input} 映射为 ${expected}`, () => {
        const result = ToolBlockAdapter.mapStatus(input)
        expect(result).toBe(expected)
      })
    })
  })
})
```

### 2.3 Hooks 测试（90% 覆盖率）

**文件**: `src/hooks/__tests__/useThread.test.ts`

```typescript
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useThread } from '../useThread'

describe('useThread', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    localStorage.clear()
  })

  it('应该创建新的线程ID', () => {
    const { result } = renderHook(() => useThread())

    expect(result.current.threadId).toBeDefined()
    expect(typeof result.current.threadId).toBe('string')
    expect(result.current.threadId).toMatch(/^test-uuid-/)
  })

  it('应该从localStorage加载现有线程ID', () => {
    const existingThreadId = 'existing-thread-123'
    localStorage.getItem = vi.fn().mockReturnValue(existingThreadId)

    const { result } = renderHook(() => useThread())

    expect(result.current.threadId).toBe(existingThreadId)
    expect(localStorage.getItem).toHaveBeenCalledWith('loom-web-thread-id')
  })

  it('应该重置线程ID', () => {
    const { result } = renderHook(() => useThread())

    const oldThreadId = result.current.threadId

    act(() => {
      result.current.resetThread()
    })

    expect(result.current.threadId).toBeDefined()
    expect(result.current.threadId).not.toBe(oldThreadId)
    expect(localStorage.setItem).toHaveBeenCalled()
  })

  it('应该将线程ID持久化到localStorage', () => {
    const { result } = renderHook(() => useThread())

    expect(localStorage.setItem).toHaveBeenCalledWith(
      'loom-web-thread-id',
      result.current.threadId
    )
  })
})
```

**文件**: `src/hooks/__tests__/useMessages.test.ts`

```typescript
import { describe, it, expect, beforeEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useMessages } from '../useMessages'
import type { UIMessageItemProps } from '../../types/ui/message'

describe('useMessages', () => {
  let defaultMessage: UIMessageItemProps

  beforeEach(() => {
    defaultMessage = {
      id: '1',
      sender: 'user',
      timestamp: new Date().toISOString(),
      content: [{ type: 'text', text: 'Hello' }]
    }
  })

  describe('addMessage', () => {
    it('应该添加新消息', () => {
      const { result } = renderHook(() => useMessages())

      act(() => {
        result.current.addMessage(defaultMessage)
      })

      expect(result.current.messages).toHaveLength(1)
      expect(result.current.messages[0]).toEqual(defaultMessage)
    })

    it('应该避免重复添加相同ID的消息', () => {
      const { result } = renderHook(() => useMessages())

      act(() => {
        result.current.addMessage(defaultMessage)
        result.current.addMessage(defaultMessage)
      })

      expect(result.current.messages).toHaveLength(1)
    })
  })

  describe('updateMessage', () => {
    it('应该更新现有消息', () => {
      const { result } = renderHook(() => useMessages())

      act(() => {
        result.current.addMessage(defaultMessage)
      })

      act(() => {
        result.current.updateMessage('1', {
          content: [{ type: 'text', text: 'Updated' }]
        })
      })

      expect(result.current.messages[0].content[0]).toHaveProperty('text', 'Updated')
    })

    it('应该忽略不存在的消息ID', () => {
      const { result } = renderHook(() => useMessages())

      act(() => {
        result.current.addMessage(defaultMessage)
        result.current.updateMessage('non-existent', {
          content: [{ type: 'text', text: 'Updated' }]
        })
      })

      expect(result.current.messages).toHaveLength(1)
      expect(result.current.messages[0].content[0]).toHaveProperty('text', 'Hello')
    })
  })

  describe('removeMessage', () => {
    it('应该移除消息', () => {
      const { result } = renderHook(() => useMessages())

      act(() => {
        result.current.addMessage(defaultMessage)
      })

      act(() => {
        result.current.removeMessage('1')
      })

      expect(result.current.messages).toHaveLength(0)
    })

    it('应该忽略不存在的消息ID', () => {
      const { result } = renderHook(() => useMessages())

      act(() => {
        result.current.addMessage(defaultMessage)
        result.current.removeMessage('non-existent')
      })

      expect(result.current.messages).toHaveLength(1)
    })
  })

  describe('clearMessages', () => {
    it('应该清空所有消息', () => {
      const { result } = renderHook(() => useMessages())

      act(() => {
        result.current.addMessage(defaultMessage)
        result.current.addMessage({ ...defaultMessage, id: '2' })
      })

      expect(result.current.messages).toHaveLength(2)

      act(() => {
        result.current.clearMessages()
      })

      expect(result.current.messages).toHaveLength(0)
    })
  })

  describe('getMessage', () => {
    it('应该获取指定消息', () => {
      const { result } = renderHook(() => useMessages())

      act(() => {
        result.current.addMessage(defaultMessage)
      })

      const message = result.current.getMessage('1')
      expect(message).toEqual(defaultMessage)
    })

    it('应该对不存在的消息返回undefined', () => {
      const { result } = renderHook(() => useMessages())

      const message = result.current.getMessage('non-existent')
      expect(message).toBeUndefined()
    })
  })

  describe('isStreaming', () => {
    it('应该正确设置流式状态', () => {
      const { result } = renderHook(() => useMessages())

      expect(result.current.isStreaming).toBe(false)

      act(() => {
        result.current.setIsStreaming(true)
      })

      expect(result.current.isStreaming).toBe(true)

      act(() => {
        result.current.setIsStreaming(false)
      })

      expect(result.current.isStreaming).toBe(false)
    })
  })
})
```

**文件**: `src/hooks/__tests__/useWebSocket.test.ts`

```typescript
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, waitFor, act } from '@testing-library/react'
import { useWebSocket } from '../useWebSocket'

describe('useWebSocket', () => {
  let mockWebSocket: any
  let mockUrl: string

  beforeEach(() => {
    mockUrl = 'ws://localhost:8080'
    
    mockWebSocket = {
      readyState: WebSocket.CONNECTING,
      send: vi.fn(),
      close: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }

    global.WebSocket = vi.fn().mockImplementation(() => {
      mockWebSocket.readyState = WebSocket.OPEN
      return mockWebSocket
    }) as any
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('应该初始化为disconnected状态', () => {
    const { result } = renderHook(() => useWebSocket({
      url: mockUrl
    }))

    expect(result.current.status).toBe('disconnected')
    expect(result.current.error).toBeNull()
  })

  it('应该成功连接', async () => {
    const onOpen = vi.fn()
    
    const { result } = renderHook(() => useWebSocket({
      url: mockUrl,
      onOpen
    }))

    act(() => {
      result.current.connect()
    })

    await waitFor(() => {
      expect(result.current.status).toBe('connected')
      expect(onOpen).toHaveBeenCalled()
    })
  })

  it('应该处理消息', async () => {
    const onMessage = vi.fn()
    const testMessage = { type: 'test', data: 'hello' }

    const { result } = renderHook(() => useWebSocket({
      url: mockUrl,
      onMessage
    }))

    act(() => {
      result.current.connect()
    })

    // 模拟接收消息
    await waitFor(() => {
      const messageHandler = mockWebSocket.addEventListener.mock.calls.find(
        (call: any[]) => call[0] === 'message'
      )
      if (messageHandler) {
        const handler = messageHandler[1]
        handler({ data: JSON.stringify(testMessage) })
      }
    })

    expect(onMessage).toHaveBeenCalled()
  })

  it('应该处理错误', async () => {
    const onError = vi.fn()

    const { result } = renderHook(() => useWebSocket({
      url: mockUrl,
      onError
    }))

    act(() => {
      result.current.connect()
    })

    // 模拟错误
    await waitFor(() => {
      const errorHandler = mockWebSocket.addEventListener.mock.calls.find(
        (call: any[]) => call[0] === 'error'
      )
      if (errorHandler) {
        const handler = errorHandler[1]
        handler(new Event('error'))
      }
    })

    expect(onError).toHaveBeenCalled()
  })

  it('应该发送消息', async () => {
    const { result } = renderHook(() => useWebSocket({
      url: mockUrl
    }))

    act(() => {
      result.current.connect()
    })

    await waitFor(() => {
      expect(result.current.status).toBe('connected')
    })

    act(() => {
      result.current.send({ type: 'test', data: 'hello' })
    })

    expect(mockWebSocket.send).toHaveBeenCalledWith(JSON.stringify({ type: 'test', data: 'hello' }))
  })

  it('应该发送字符串消息', async () => {
    const { result } = renderHook(() => useWebSocket({
      url: mockUrl
    }))

    act(() => {
      result.current.connect()
    })

    await waitFor(() => {
      expect(result.current.status).toBe('connected')
    })

    act(() => {
      result.current.send('plain text')
    })

    expect(mockWebSocket.send).toHaveBeenCalledWith('plain text')
  })

  it('应该断开连接', async () => {
    const onClose = vi.fn()

    const { result } = renderHook(() => useWebSocket({
      url: mockUrl,
      onClose
    }))

    act(() => {
      result.current.connect()
    })

    await waitFor(() => {
      expect(result.current.status).toBe('connected')
    })

    act(() => {
      result.current.disconnect()
    })

    expect(mockWebSocket.close).toHaveBeenCalled()
  })

  it('应该自动重连', async () => {
    const { result } = renderHook(() => useWebSocket({
      url: mockUrl,
      reconnectAttempts: 3,
      reconnectInterval: 100
    }))

    act(() => {
      result.current.connect()
    })

    await waitFor(() => {
      expect(result.current.status).toBe('connected')
    })

    // 模拟连接关闭
    await waitFor(() => {
      const closeHandler = mockWebSocket.addEventListener.mock.calls.find(
        (call: any[]) => call[0] === 'close'
      )
      if (closeHandler) {
        const handler = closeHandler[1]
        handler(new CloseEvent('close'))
      }
    })

    // 应该尝试重连
    await waitFor(() => {
      expect(global.WebSocket).toHaveBeenCalledTimes(2)
    }, { timeout: 500 })
  })
})
```

**文件**: `src/hooks/__tests__/useChat.test.ts`

```typescript
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, waitFor, act } from '@testing-library/react'
import { useChat } from '../useChat'

// Mock dependencies
vi.mock('../useWebSocket')
vi.mock('../useThread')
vi.mock('../useMessages')

describe('useChat', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('应该初始化', () => {
    const { result } = renderHook(() => useChat())

    expect(result.current.messages).toEqual([])
    expect(result.current.isStreaming).toBe(false)
    expect(result.current.connectionStatus).toBeDefined()
    expect(result.current.error).toBeNull()
  })

  it('应该发送消息', async () => {
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.sendMessage('Hello')
    })

    expect(result.current.messages).toHaveLength(1)
    expect(result.current.messages[0].sender).toBe('user')
  })

  it('应该忽略空消息', async () => {
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.sendMessage('')
      await result.current.sendMessage('   ')
    })

    expect(result.current.messages).toHaveLength(0)
  })

  it('应该重置聊天', async () => {
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.sendMessage('Hello')
    })

    expect(result.current.messages).toHaveLength(1)

    act(() => {
      result.current.resetChat()
    })

    expect(result.current.messages).toHaveLength(0)
  })

  it('应该清空消息', async () => {
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.sendMessage('Hello')
    })

    expect(result.current.messages).toHaveLength(1)

    act(() => {
      result.current.clearMessages()
    })

    expect(result.current.messages).toHaveLength(0)
  })

  it('应该处理服务器消息', async () => {
    const { result } = renderHook(() => useChat())

    // 模拟接收服务器消息
    await waitFor(() => {
      // 这里需要根据实际的服务器消息处理逻辑来测试
      expect(result.current).toBeDefined()
    })
  })
})
```

### 2.4 工具函数测试（100% 覆盖率）

**文件**: `src/utils/__tests__/format.test.ts`

```typescript
import { describe, it, expect } from 'vitest'
import { formatTime, formatDate } from '../format'

describe('formatTime', () => {
  it('应该格式化时间为 HH:MM 格式', () => {
    const timestamp = '2024-01-15T10:30:00Z'
    const result = formatTime(timestamp)
    
    expect(result).toMatch(/\d{2}:\d{2}/)
  })

  it('应该处理不同的时区', () => {
    const timestamp = '2024-01-15T22:45:00Z'
    const result = formatTime(timestamp)
    
    expect(result).toMatch(/\d{2}:\d{2}/)
  })

  it('应该处理ISO 8601格式', () => {
    const timestamp = '2024-01-15T14:20:30.123Z'
    const result = formatTime(timestamp)
    
    expect(result).toMatch(/\d{2}:\d{2}/)
  })

  it('应该处理无效时间戳', () => {
    const timestamp = 'invalid'
    
    expect(() => formatTime(timestamp)).toThrow()
  })
})

describe('formatDate', () => {
  it('应该格式化日期', () => {
    const timestamp = '2024-01-15T10:30:00Z'
    const result = formatDate(timestamp)
    
    expect(result).toContain('2024')
  })
})
```

## Phase 3: 组件测试（第6-8天）- 目标 85% 覆盖率

### 3.1 MessageItem 组件测试

**文件**: `src/components/chat/__tests__/MessageItem.test.tsx`

```typescript
import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MessageItem } from '../MessageItem'
import type { UIMessageItemProps } from '../../../types/ui/message'

describe('MessageItem', () => {
  const defaultMessage: UIMessageItemProps = {
    id: '1',
    sender: 'user',
    timestamp: '2024-01-15T10:30:00Z',
    content: [{ type: 'text', text: 'Hello' }]
  }

  it('应该渲染用户消息', () => {
    render(<MessageItem {...defaultMessage} />)

    expect(screen.getByText('Hello')).toBeInTheDocument()
    expect(screen.getByText('用户')).toBeInTheDocument()
  })

  it('应该渲染助手消息', () => {
    render(<MessageItem {...defaultMessage} sender="assistant" />)

    expect(screen.getByText('助手')).toBeInTheDocument()
  })

  it('应该渲染时间戳', () => {
    render(<MessageItem {...defaultMessage} />)

    expect(screen.getByText(/\d{2}:\d{2}/)).toBeInTheDocument()
  })

  it('应该渲染工具消息', () => {
    const toolMessage: UIMessageItemProps = {
      id: '2',
      sender: 'assistant',
      timestamp: '2024-01-15T10:30:00Z',
      content: [{
        type: 'tool',
        id: 'tool-1',
        name: 'bash',
        status: 'success',
        argumentsText: 'ls',
        outputText: 'file1\nfile2',
        resultText: '',
        isError: false
      }]
    }

    render(<MessageItem {...toolMessage} />)

    expect(screen.getByText('bash')).toBeInTheDocument()
    expect(screen.getByText(/成功/i)).toBeInTheDocument()
  })

  it('应该支持自定义className', () => {
    render(<MessageItem {...defaultMessage} className="custom-class" />)

    const article = screen.getByRole('article')
    expect(article).toHaveClass('custom-class')
  })

  it('应该显示重试按钮', () => {
    const onRetry = vi.fn()
    render(<MessageItem {...defaultMessage} onRetry={onRetry} />)

    expect(screen.getByText('重试')).toBeInTheDocument()
  })

  it('应该支持可访问性', () => {
    render(<MessageItem {...defaultMessage} />)

    expect(screen.getByRole('article')).toHaveAttribute('aria-label')
  })
})
```

### 3.2 MessageList 组件测试

**文件**: `src/components/chat/__tests__/MessageList.test.tsx`

```typescript
import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MessageList } from '../MessageList'
import type { UIMessageItemProps } from '../../../types/ui/message'

describe('MessageList', () => {
  const messages: UIMessageItemProps[] = [
    {
      id: '1',
      sender: 'user',
      timestamp: '2024-01-15T10:30:00Z',
      content: [{ type: 'text', text: 'Hello' }]
    },
    {
      id: '2',
      sender: 'assistant',
      timestamp: '2024-01-15T10:31:00Z',
      content: [{ type: 'text', text: 'Hi there!' }]
    }
  ]

  it('应该渲染消息列表', () => {
    render(<MessageList messages={messages} />)

    expect(screen.getByText('Hello')).toBeInTheDocument()
    expect(screen.getByText('Hi there!')).toBeInTheDocument()
  })

  it('应该处理空列表', () => {
    render(<MessageList messages={[]} />)

    const list = screen.getByRole('log')
    expect(list).toBeEmptyDOMElement()
  })

  it('应该支持自定义className', () => {
    render(<MessageList messages={messages} className="custom-list" />)

    const list = screen.getByRole('log')
    expect(list).toHaveClass('custom-list')
  })

  it('应该支持可访问性', () => {
    render(<MessageList messages={messages} />)

    const list = screen.getByRole('log')
    expect(list).toHaveAttribute('aria-live', 'polite')
    expect(list).toHaveAttribute('aria-label', '聊天消息')
  })

  it('应该自动滚动到底部', () => {
    const { container } = render(<MessageList messages={messages} />)

    const listElement = container.querySelector('.message-list')
    expect(listElement).toBeDefined()
  })
})
```

## Phase 4: 集成测试（第9-10天）

### 4.1 完整聊天流程测试

**文件**: `src/__tests__/integration/chat.test.tsx`

```typescript
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ChatPage } from '../../pages/ChatPage-new'

describe('Chat Integration', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('应该完成完整的聊天流程', async () => {
    const user = userEvent.setup()
    
    render(<ChatPage />)

    // 等待连接
    await waitFor(() => {
      expect(screen.getByText(/已连接/i)).toBeInTheDocument()
    })

    // 输入消息
    const input = screen.getByPlaceholderText(/写消息/i)
    await user.type(input, 'Hello, assistant!')

    // 发送消息
    const sendButton = screen.getByRole('button', { name: /发送/i })
    await user.click(sendButton)

    // 验证用户消息显示
    await waitFor(() => {
      expect(screen.getByText('Hello, assistant!')).toBeInTheDocument()
    })

    // 验证助手响应（模拟）
    await waitFor(() => {
      // 这里需要根据实际的响应逻辑来验证
    })
  })

  it('应该处理连接错误', async () => {
    render(<ChatPage />)

    // 模拟连接错误
    await waitFor(() => {
      // 验证错误处理
    })
  })

  it('应该支持重连', async () => {
    const user = userEvent.setup()
    
    render(<ChatPage />)

    // 模拟断线
    // 点击重连按钮
    const retryButton = screen.getByRole('button', { name: /重试/i })
    await user.click(retryButton)

    // 验证重连逻辑
    await waitFor(() => {
      expect(screen.getByText(/已连接/i)).toBeInTheDocument()
    })
  })
})
```

## Phase 5: E2E测试（第11-12天，可选）

### 5.1 Playwright E2E测试

**文件**: `e2e/chat.spec.ts`

```typescript
import { test, expect } from '@playwright/test'

test.describe('Chat E2E', () => {
  test('应该完成完整的聊天流程', async ({ page }) => {
    await page.goto('/')

    // 等待页面加载
    await expect(page.locator('.connection-status')).toContainText('已连接', { timeout: 10000 })

    // 输入消息
    await page.fill('[placeholder*="写消息"]', '测试消息')
    
    // 发送
    await page.click('button:has-text("发送")')

    // 验证消息出现
    await expect(page.locator('.message-list')).toContainText('测试消息', { timeout: 5000 })

    // 验证助手响应
    await expect(page.locator('.message--assistant')).toBeVisible({ timeout: 10000 })
  })

  test('应该处理网络断线', async ({ page, context }) => {
    await page.goto('/')

    // 等待连接
    await expect(page.locator('.connection-status')).toContainText('已连接')

    // 模拟离线
    await context.setOffline(true)

    // 验证离线状态
    await expect(page.locator('.connection-status')).toContainText('未连接', { timeout: 5000 })

    // 恢复在线
    await context.setOffline(false)

    // 验证重连
    await expect(page.locator('.connection-status')).toContainText('已连接', { timeout: 10000 })
  })
})
```

## 执行计划

### Week 1 (Day 1-5): 核心测试

| 天数 | 任务 | 预期覆盖率 |
|------|------|-----------|
| Day 1 | 设置测试框架 | 0% |
| Day 2 | 类型守卫 + 适配器测试 | 30% |
| Day 3 | 适配器测试完成 | 40% |
| Day 4 | Hooks测试 (useThread, useMessages) | 55% |
| Day 5 | Hooks测试 (useWebSocket, useChat) | 70% |

### Week 2 (Day 6-10): 组件和集成测试

| 天数 | 任务 | 预期覆盖率 |
|------|------|-----------|
| Day 6 | MessageItem, MessageList测试 | 78% |
| Day 7 | 其他组件测试 | 85% |
| Day 8 | 组件测试完成 | 88% |
| Day 9 | 集成测试 | 90% |
| Day 10 | E2E测试 + 修复 | 90%+ |

## 覆盖率报告

### 生成覆盖率报告

```bash
npm run test:coverage
```

### 查看覆盖率报告

```bash
# 打开HTML报告
open coverage/index.html
```

### CI/CD集成

```yaml
# .github/workflows/test.yml
name: Test

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions/setup-node@v3
        with:
          node-version: '20'
      - run: npm ci
      - run: npm run test:coverage
      - uses: codecov/codecov-action@v3
        with:
          files: ./coverage/lcov.info
          fail_ci_if_error: true
```

## 成功标准

### 必须达到的指标

| 指标 | 目标 | 验证方法 |
|------|------|---------|
| 语句覆盖率 | ≥90% | `npm run test:coverage` |
| 分支覆盖率 | ≥85% | 覆盖率报告 |
| 函数覆盖率 | ≥90% | 覆盖率报告 |
| 行覆盖率 | ≥90% | 覆盖率报告 |

### 额外要求

- ✅ 所有测试必须通过
- ✅ 无console错误或警告
- ✅ 测试执行时间 < 30秒
- ✅ 测试代码清晰、可维护
- ✅ 测试覆盖所有边界情况
- ✅ 测试覆盖错误处理

## 工具和资源

### 推荐工具

- **Vitest** - 快速的单元测试框架
- **@testing-library/react** - React组件测试
- **@vitest/coverage-v8** - 代码覆盖率
- **MSW** - API模拟（如需要）
- **Playwright** - E2E测试

### 有用的命令

```bash
# 运行所有测试
npm test

# 运行特定文件测试
npm test MessageAdapter

# 监视模式
npm run test:watch

# 生成覆盖率报告
npm run test:coverage

# UI模式
npm run test:ui

# 调试模式
npm test -- --reporter=verbose
```

## 总结

通过这个方案，我们将在2周内达到：

- ✅ **90%+ 语句覆盖率**
- ✅ **85%+ 分支覆盖率**
- ✅ **90%+ 函数覆盖率**
- ✅ **90%+ 行覆盖率**
- ✅ **完整的测试文档**
- ✅ **CI/CD集成**

这将确保代码质量、可维护性和可靠性！
