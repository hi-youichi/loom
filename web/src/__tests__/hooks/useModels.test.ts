import { describe, it, expect, beforeEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'

const mockModels = [
  { id: 'model-0', name: 'Model 0', provider: 'test-provider' },
  { id: 'model-1', name: 'Model 1', provider: 'test-provider' },
]

function createMockConnection() {
  const eventHandlers: Record<string, Set<Function>> = {}

  return {
    on: vi.fn((event: string, handler: Function) => {
      if (!eventHandlers[event]) eventHandlers[event] = new Set()
      eventHandlers[event].add(handler)
    }),
    off: vi.fn((event: string, handler: Function) => {
      eventHandlers[event]?.delete(handler)
    }),
    request: vi.fn().mockResolvedValue({ models: mockModels }),
    emit(event: string, data: unknown) {
      eventHandlers[event]?.forEach(fn => fn(data))
    },
  }
}

const mockConn = createMockConnection()

vi.mock('../../services/connection', () => ({
  getConnection: () => mockConn,
}))

describe('useModels', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    localStorage.clear()
    mockConn.on.mockClear()
    mockConn.off.mockClear()
    mockConn.request.mockResolvedValue({ models: mockModels })
  })

  it('should subscribe to models_updated on mount', async () => {
    const { useModels } = await import('../../hooks/useModels')
    renderHook(() => useModels())

    expect(mockConn.on).toHaveBeenCalledWith('models_updated', expect.any(Function))
  })

  it('should initialize with empty models when no cache', async () => {
    const { useModels } = await import('../../hooks/useModels')
    const { result } = renderHook(() => useModels())

    expect(result.current.models).toEqual([])
  })

  it('should update models when models_updated event fires', async () => {
    const { useModels } = await import('../../hooks/useModels')
    const { result } = renderHook(() => useModels())

    const newModels = [
      { id: 'new-1', name: 'New 1', provider: 'p' },
      { id: 'new-2', name: 'New 2', provider: 'p' },
    ]

    act(() => {
      mockConn.emit('models_updated', newModels)
    })

    expect(result.current.models).toEqual(newModels)
  })

  it('should set loading to false after models_updated event', async () => {
    const { useModels } = await import('../../hooks/useModels')
    const { result } = renderHook(() => useModels())

    act(() => {
      mockConn.emit('models_updated', mockModels)
    })

    expect(result.current.loading).toBe(false)
  })

  it('should clear error after models_updated event', async () => {
    const { useModels } = await import('../../hooks/useModels')
    const { result } = renderHook(() => useModels())

    act(() => {
      mockConn.emit('models_updated', mockModels)
    })

    expect(result.current.error).toBeNull()
  })

  it('should unsubscribe on unmount', async () => {
    const { useModels } = await import('../../hooks/useModels')
    const { unmount } = renderHook(() => useModels())

    const handler = mockConn.on.mock.calls.find(
      (call: unknown[]) => call[0] === 'models_updated'
    )?.[1] as Function

    expect(handler).toBeDefined()

    unmount()

    expect(mockConn.off).toHaveBeenCalledWith('models_updated', handler)
  })

  it('should refetch models via refetch function', async () => {
    const freshModels = [
      { id: 'fresh-1', name: 'Fresh 1', provider: 'p' },
    ]
    mockConn.request.mockResolvedValueOnce({ models: freshModels })

    const { useModels } = await import('../../hooks/useModels')
    const { result } = renderHook(() => useModels())

    await act(async () => {
      await result.current.refetch()
    })

    expect(mockConn.request).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'list_models' })
    )
    expect(result.current.models).toEqual(freshModels)
  })

  it('should set error when refetch fails', async () => {
    mockConn.request.mockRejectedValueOnce(new Error('Network error'))

    const { useModels } = await import('../../hooks/useModels')
    const { result } = renderHook(() => useModels())

    await act(async () => {
      await result.current.refetch()
    })

    expect(result.current.error).toBe('Network error')
  })
})
