import { describe, it, expect, beforeEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'

describe('useSessionId', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('generates and stores session ID', async () => {
    const { useSessionId } = await import('../../hooks/useSessionId')
    const { result } = renderHook(() => useSessionId())
    expect(result.current.sessionId).toBeTruthy()
  })

  it('setSessionId updates session ID', async () => {
    const { useSessionId } = await import('../../hooks/useSessionId')
    const { result } = renderHook(() => useSessionId())

    act(() => result.current.setSessionId('updated-id'))
    expect(result.current.sessionId).toBe('updated-id')
  })

  it('resetSession generates new session ID', async () => {
    const { useSessionId } = await import('../../hooks/useSessionId')
    const { result } = renderHook(() => useSessionId())

    const oldId = result.current.sessionId
    let newId: string = ''
    act(() => { newId = result.current.resetSession() })

    expect(newId).not.toBe(oldId)
    expect(result.current.sessionId).toBe(newId)
  })
})
