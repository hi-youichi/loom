import { describe, it, expect, beforeEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'

const mockModels = [
  { id: 'model-0', name: 'Model 0', provider: 'test-provider' },
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

describe('useModels caching', () => {
  beforeEach(() => {
    localStorage.clear()
    mockConn.request.mockResolvedValue({ models: mockModels })
  })

  it('sets loading to true when no cache exists', async () => {
    const { useModels } = await import('../../hooks/useModels')
    const { result } = renderHook(() => useModels())
    expect(result.current.loading).toBe(true)
  })

  it('handles localStorage write failure gracefully', async () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('Storage full')
    })

    const { useModels } = await import('../../hooks/useModels')
    const { result } = renderHook(() => useModels())

    expect(() => {
      act(() => mockConn.emit('models_updated', [{ id: '1', name: 'M', provider: 'p' }]))
    }).not.toThrow()

    vi.restoreAllMocks()
  })
})
