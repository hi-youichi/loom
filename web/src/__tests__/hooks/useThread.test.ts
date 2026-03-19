import { describe, it, expect, beforeEach } from 'vitest'
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
})
