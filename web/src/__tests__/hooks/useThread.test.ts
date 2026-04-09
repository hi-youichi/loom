import { describe, it, expect, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useThread } from '../../hooks/useThread'

describe('useThread', () => {
  it('应该创建新的线程ID', () => {
    const { result } = renderHook(() => useThread())
    expect(result.current.threadId).toBeDefined()
    expect(typeof result.current.threadId).toBe('string')
    expect(result.current.threadId.length).toBeGreaterThan(0)
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

  it('resetThread 应该是一个函数', () => {
    const { result } = renderHook(() => useThread())
    expect(typeof result.current.resetThread).toBe('function')
  })

  it('应该从localStorage恢复已有的线程ID', () => {
    const existingId = 'my-existing-thread-id'
    vi.mocked(window.localStorage.getItem).mockReturnValue(existingId)

    const { result } = renderHook(() => useThread())
    expect(result.current.threadId).toBe(existingId)

    vi.mocked(window.localStorage.getItem).mockRestore()
  })

  it('新线程ID应该调用localStorage.setItem', () => {
    vi.mocked(window.localStorage.getItem).mockReturnValue(null)

    const { result } = renderHook(() => useThread())
    expect(window.localStorage.setItem).toHaveBeenCalledWith(
      'loom-web-thread-id',
      result.current.threadId
    )
  })

  it('resetThread应该调用localStorage.setItem', () => {
    vi.mocked(window.localStorage.getItem).mockReturnValue(null)
    vi.mocked(window.localStorage.setItem).mockClear()

    const { result } = renderHook(() => useThread())

    act(() => {
      result.current.resetThread()
    })
    expect(window.localStorage.setItem).toHaveBeenCalledWith(
      'loom-web-thread-id',
      result.current.threadId
    )
  })
})
