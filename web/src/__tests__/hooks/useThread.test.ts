import { describe, it, expect, beforeEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'

describe('useThread', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('generates and stores thread ID', async () => {
    const { useThread } = await import('../../hooks/useThread')
    const { result } = renderHook(() => useThread())
    expect(result.current.threadId).toBeTruthy()
  })

  it('setThreadId updates thread ID', async () => {
    const { useThread } = await import('../../hooks/useThread')
    const { result } = renderHook(() => useThread())

    act(() => result.current.setThreadId('updated-id'))
    expect(result.current.threadId).toBe('updated-id')
  })

  it('resetThread generates new thread ID', async () => {
    const { useThread } = await import('../../hooks/useThread')
    const { result } = renderHook(() => useThread())

    const oldId = result.current.threadId
    let newId: string = ''
    act(() => { newId = result.current.resetThread() })

    expect(newId).not.toBe(oldId)
    expect(result.current.threadId).toBe(newId)
  })
})
