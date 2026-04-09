import { describe, it, expect } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useChatPanel } from '../../hooks/useChatPanel'

describe('useChatPanel', () => {
  beforeEach(() => {
    localStorage.removeItem('chatPanelState')
  })

  it('should initialize with default state', () => {
    const { result } = renderHook(() => useChatPanel())

    expect(result.current.collapsed).toBe(false)
    expect(result.current.width).toBe(400)
    expect(result.current.selectedAgentId).toBeNull()
  })

  it('should toggle collapsed state', () => {
    const { result } = renderHook(() => useChatPanel())

    expect(result.current.collapsed).toBe(false)

    act(() => {
      result.current.toggle()
    })

    expect(result.current.collapsed).toBe(true)
  })

  it('should expand when toggle is called on collapsed state', () => {
    const { result } = renderHook(() => useChatPanel())

    act(() => {
      result.current.toggle()
      result.current.toggle()
    })

    expect(result.current.collapsed).toBe(false)
  })

  it('should collapse when toggle is called on expanded state', () => {
    const { result } = renderHook(() => useChatPanel())

    act(() => {
      result.current.toggle()
    })

    expect(result.current.collapsed).toBe(true)
  })

  it('should collapse when width is set below threshold', () => {
    const { result } = renderHook(() => useChatPanel())

    act(() => {
      result.current.setWidth(150)
    })

    expect(result.current.collapsed).toBe(true)
    expect(result.current.width).toBe(320) // clamped to minimum width
  })

  it('should expand when width is set above threshold', () => {
    const { result } = renderHook(() => useChatPanel())

    act(() => {
      result.current.toggle() // collapse first
      result.current.setWidth(500)
    })

    expect(result.current.collapsed).toBe(false)
  })

  it('should select agent', () => {
    const { result } = renderHook(() => useChatPanel())

    expect(result.current.selectedAgentId).toBeNull()

    act(() => {
      result.current.selectAgent('dev')
    })

    expect(result.current.selectedAgentId).toBe('dev')
  })

  it('should clear selected agent when reset is called', () => {
    const { result } = renderHook(() => useChatPanel())

    act(() => {
      result.current.selectAgent('dev')
      result.current.reset()
    })

    expect(result.current.selectedAgentId).toBeNull()
  })

  it('should collapse and reset when reset is called', () => {
    const { result } = renderHook(() => useChatPanel())

    act(() => {
      result.current.selectAgent('dev')
      result.current.setWidth(150)
      result.current.reset()
    })

    expect(result.current.collapsed).toBe(false)
    expect(result.current.width).toBe(400)
    expect(result.current.selectedAgentId).toBeNull()
  })
})
