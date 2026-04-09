/**
 * 测试设置文件
 * 在所有测试之前运行
 */
import '@testing-library/jest-dom'

// Mock localStorage
const localStorageMock = {
  getItem: vi.fn(),
  setItem: vi.fn(),
  removeItem: vi.fn(),
  clear: vi.fn(),
}
Object.defineProperty(window, 'localStorage', {
  value: localStorageMock,
})

// Mock crypto.randomUUID
Object.defineProperty(window, 'crypto', {
  value: {
    randomUUID: () => 'test-uuid-' + Math.random().toString(36).substr(2, 9),
  },
})

// Mock WebSocket
class MockWebSocket {
  static CONNECTING = 0
  static OPEN = 1
  static CLOSING = 2
  static CLOSED = 3

  readyState = MockWebSocket.OPEN
  onopen: (() => void) | null = null
  onclose: (() => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  onerror: ((error: Event) => void) | null = null

  constructor(public url: string) {
    setTimeout(() => {
      this.onopen?.()
    }, 0)
  }

  send(data: string) {
    console.log('WebSocket send:', data)
  }

  close() {
    this.readyState = MockWebSocket.CLOSED
    this.onclose?.()
  }
}

// @ts-expect-error: MockWebSocket needs to replace native WebSocket
window.WebSocket = MockWebSocket

// 全局测试工具
global.console = {
  ...console,
  error: vi.fn(),
  warn: vi.fn(),
}
