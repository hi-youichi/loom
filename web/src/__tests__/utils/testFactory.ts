/**
 * 测试数据工厂
 * 用于生成测试数据的辅助函数
 */

import type { 
  LoomMessageChunkEvent,
  LoomThoughtChunkEvent,
  LoomToolCallEvent,
  LoomRunStreamEventResponse,
  LoomRunEndResponse,
  LoomErrorResponse
} from '../../types/protocol/loom'
import type { UIMessageItemProps, UITextContent, UIToolContent } from '../types/ui/message'

/**
 * 创建用户事件
 */
export function createLoomMessageChunkEvent(
  overrides: Partial<LoomMessageChunkEvent> = {},
): LoomMessageChunkEvent {
  return {
    type: 'message_chunk',
    id: 'think',
    content: 'Hello',
    ...overrides,
  }
}

/**
 * 创建思考片段事件 (thought_chunk，对应 ACP agent_thought_chunk)
 */
export function createLoomThoughtChunkEvent(
  overrides: Partial<LoomThoughtChunkEvent> = {},
): LoomThoughtChunkEvent {
  return {
    type: 'thought_chunk',
    id: 'think',
    content: 'Consider the options first.',
    ...overrides,
  }
}

/**
 * 创建助手工具事件
 */
export function createLoomToolCallEvent(overrides: Partial<LoomToolCallEvent> = {}): LoomToolCallEvent {
  return {
    type: 'tool_call',
    call_id: 'call-1',
    name: 'test_tool',
    arguments: { arg: 'value' },
    ...overrides,
  }
}

/**
 * 创建流事件响应
 */
export function createRunStreamEventResponse(
  event: LoomMessageChunkEvent | LoomThoughtChunkEvent | LoomToolCallEvent
): LoomRunStreamEventResponse {
  return {
    type: 'run_stream_event',
    id: 'response-1',
    event,
  }
}

/**
 * 创建运行结束响应
 */
export function createRunEndResponse(overrides: Partial<LoomRunEndResponse> = {}): LoomRunEndResponse {
  return {
    type: 'run_end',
    id: 'run-1',
    reply: 'Done',
    ...overrides,
  }
}

/**
 * 创建错误响应
 */
export function createErrorResponse(overrides: Partial<LoomErrorResponse> = {}): LoomErrorResponse {
  return {
    type: 'error',
    id: 'error-1',
    error: 'Something went wrong',
    ...overrides,
  }
}

/**
 * 创建UI消息
 */
export function createUIMessage(overrides: Partial<UIMessageItemProps> = {}): UIMessageItemProps {
  return {
    id: 'msg-1',
    sender: 'user',
    timestamp: new Date().toISOString(),
    content: [createUITextContent()],
    ...overrides,
  }
}

/**
 * 创建UI文本内容
 */
export function createUITextContent(overrides: Partial<UITextContent> = {}): UITextContent {
  return {
    type: 'text',
    text: 'Test message',
    format: 'plain',
    ...overrides,
  }
}

/**
 * 创建UI工具内容
 */
export function createUIToolContent(overrides: Partial<UIToolContent> = {}): UIToolContent {
  return {
    type: 'tool',
    id: 'tool-1',
    name: 'test_tool',
    status: 'success',
    argumentsText: '{"arg": "value"}',
    outputText: 'Tool output',
    resultText: 'Tool result',
    isError: false,
    ...overrides,
  }
}

/**
 * 创建消息列表
 */
export function createUIMessageList(count: number = 3): UIMessageItemProps[] {
  return Array.from({ length: count }, (_, i) => 
    createUIMessage({
      id: `msg-${i + 1}`,
      sender: i % 2 === 0 ? 'user' : 'assistant',
      content: [createUITextContent({ text: `Message ${i + 1}` })],
    })
  )
}

/**
 * 创建混合内容消息（文本+工具）
 */
export function createMixedContentMessage(): UIMessageItemProps {
  return {
    id: 'msg-mixed',
    sender: 'assistant',
    timestamp: new Date().toISOString(),
    content: [
      createUITextContent({ text: 'Let me help you with that.' }),
      createUIToolContent({ name: 'search', status: 'running' }),
      createUIToolContent({ name: 'search', status: 'success' }),
      createUITextContent({ text: 'Here are the results.' }),
    ],
  }
}

/**
 * 模拟WebSocket消息事件
 */
export function createMessageEvent(data: unknown): MessageEvent {
  return new MessageEvent('message', {
    data: JSON.stringify(data),
  })
}

/**
 * 等待指定毫秒
 */
export function wait(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/**
 * 创建模拟的WebSocket
 */
export function createMockWebSocket(): {
  socket: WebSocket
  send: vi.Mock
  close: vi.Mock
  triggerOpen: () => void
  triggerMessage: (data: unknown) => void
  triggerError: (error: Error) => void
  triggerClose: () => void
} {
  const listeners = {
    open: [] as EventListener[],
    message: [] as EventListener[],
    error: [] as EventListener[],
    close: [] as EventListener[],
  }

  const send = vi.fn()
  const close = vi.fn()

  const socket = {
    readyState: WebSocket.CONNECTING,
    send,
    close,
    addEventListener: vi.fn((event: string, listener: EventListener) => {
      listeners[event as keyof typeof listeners]?.push(listener)
    }),
    removeEventListener: vi.fn((event: string, listener: EventListener) => {
      const arr = listeners[event as keyof typeof listeners]
      if (arr) {
        const index = arr.indexOf(listener)
        if (index > -1) arr.splice(index, 1)
      }
    }),
  } as unknown as WebSocket

  return {
    socket,
    send,
    close,
    triggerOpen: () => {
      socket.readyState = WebSocket.OPEN
      listeners.open.forEach(l => l(new Event('open')))
    },
    triggerMessage: (data: unknown) => {
      const event = createMessageEvent(data)
      listeners.message.forEach((listener) => listener(event))
    },
    triggerError: () => {
      const event = new Event('error') as ErrorEvent
      listeners.error.forEach((listener) => listener(event))
    },
    triggerClose: () => {
      socket.readyState = WebSocket.CLOSED
      listeners.close.forEach((listener) => listener(new Event('close')))
    },
  }
}
