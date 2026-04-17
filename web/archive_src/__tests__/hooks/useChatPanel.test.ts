import { describe, it, expect, beforeEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'

describe('useChatPanel', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('returns default state', async () => {
    const { useChatPanel } = await import('../../hooks/useChatPanel')
    const { result } = renderHook(() => useChatPanel())
    expect(result.current.collapsed).toBe(false)
    expect(result.current.width).toBe(400)
    expect(result.current.selectedAgentId).toBeNull()
  })

  it('toggles collapsed', async () => {
    const { useChatPanel } = await import('../../hooks/useChatPanel')
    const { result } = renderHook(() => useChatPanel())

    act(() => result.current.toggle())
    expect(result.current.collapsed).toBe(true)

    act(() => result.current.toggle())
    expect(result.current.collapsed).toBe(false)
  })

  it('expands', async () => {
    const { useChatPanel } = await import('../../hooks/useChatPanel')
    const { result } = renderHook(() => useChatPanel())

    act(() => result.current.collapse())
    expect(result.current.collapsed).toBe(true)

    act(() => result.current.expand())
    expect(result.current.collapsed).toBe(false)
  })

  it('collapses', async () => {
    const { useChatPanel } = await import('../../hooks/useChatPanel')
    const { result } = renderHook(() => useChatPanel())

    act(() => result.current.collapse())
    expect(result.current.collapsed).toBe(true)
  })

  it('clamps width to min 320', async () => {
    const { useChatPanel } = await import('../../hooks/useChatPanel')
    const { result } = renderHook(() => useChatPanel())

    act(() => result.current.setWidth(100))
    expect(result.current.width).toBe(320)
    expect(result.current.collapsed).toBe(true)
  })

  it('clamps width to max 600', async () => {
    const { useChatPanel } = await import('../../hooks/useChatPanel')
    const { result } = renderHook(() => useChatPanel())

    act(() => result.current.setWidth(800))
    expect(result.current.width).toBe(600)
  })

  it('selects agent', async () => {
    const { useChatPanel } = await import('../../hooks/useChatPanel')
    const { result } = renderHook(() => useChatPanel())

    act(() => result.current.selectAgent('agent-1'))
    expect(result.current.selectedAgentId).toBe('agent-1')
  })

  it('resets to default state', async () => {
    const { useChatPanel } = await import('../../hooks/useChatPanel')
    const { result } = renderHook(() => useChatPanel())

    act(() => result.current.collapse())
    act(() => result.current.selectAgent('agent-1'))

    act(() => result.current.reset())
    expect(result.current.collapsed).toBe(false)
    expect(result.current.width).toBe(400)
    expect(result.current.selectedAgentId).toBeNull()
  })
})
