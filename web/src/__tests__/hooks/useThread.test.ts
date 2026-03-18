import { describe, it, expect, beforeEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useThread } from '../../hooks/useThread'

// Mock localStorage
const localStorageMock = (() => {
  let store: Record<string, string> = {}
  return {
    getItem: vi.fn((key: string) => store[key] || null),
    setItem: vi.fn((key: string, value: string) => {
      store[key] = value
    }),
    removeItem: vi.fn((key: string) => {
      delete store[key]
    }),
    clear: vi.fn(() => {
      store = {}
    }),
  }
})()

Object.defineProperty(window, 'localStorage', {
  value: localStorageMock,
})

describe('useThread', () => {
  beforeEach(() => {
    localStorageMock.clear()
    vi.clearAllMocks()
  })

  it('应该创建新的线程ID', () => {
    const { result } = renderHook(() => useThread())

    expect(result.current.threadId).toBeDefined()
    expect(result.current.threadId).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
    )
  })

  it('应该从localStorage恢复线程ID', () => {
    const existingThreadId = 'existing-thread-id'
    localStorageMock.getItem.mockReturnValue(existingThreadId)

    const { result } = renderHook(() => useThread())

    expect(result.current.threadId).toBe(existingThreadId)
    expect(localStorageMock.getItem).toHaveBeenCalledWith('loom-web-thread-id')
  })

  it('应该重置线程ID', () => {
    const { result } = renderHook(() => useThread())
    const oldThreadId = result.current.threadId

    act(() => {
      result.current.resetThread()
    })

    expect(result.current.threadId).toBeDefined()
    expect(result.current.threadId).not.toBe(oldThreadId)
  })

  it('应该在重置时保存新线程ID到localStorage', () => {
    const { result } = renderHook(() => useThread())

    act(() => {
      result.current.resetThread()
    })

    expect(localStorageMock.setItem).toHaveBeenCalledWith(
      'loom-web-thread-id',
      result.current.threadId
    )
  })

  it('应该在初始化时保存线程ID到localStorage', () => {
    renderHook(() => useThread())

    expect(localStorageMock.setItem).toHaveBeenCalled()
  })

  it('多个hook实例应该共享同一个线程ID', () => {
    const existingThreadId = 'shared-thread-id'
    localStorageMock.getItem.mockReturnValue(existingThreadId)

    const { result: result1 } = renderHook(() => useThread())
    const { result: result2 } = renderHook(() => useThread())

    expect(result1.current.threadId).toBe(result2.current.threadId)
  })
})
