import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { getConnection, closeConnection, type Model } from '../../services/connection'

describe('LoomConnection event bus', () => {
  let sentData: string[]
  let addEventListenerFn: vi.Mock
  let originalWebSocket: typeof globalThis.WebSocket
  let mockWs: Record<string, any>

  beforeEach(() => {
    closeConnection()
    sentData = []
    addEventListenerFn = vi.fn()

    mockWs = {
      readyState: 0,
      send: (data: string) => { sentData.push(data) },
      close: vi.fn(),
      addEventListener: addEventListenerFn,
      removeEventListener: vi.fn(),
    }

    originalWebSocket = globalThis.WebSocket

    function MockWebSocket(this: any) {
      return mockWs
    }
    vi.stubGlobal('WebSocket', MockWebSocket)
    Object.assign(globalThis.WebSocket, { CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 })
  })

  afterEach(() => {
    closeConnection()
    globalThis.WebSocket = originalWebSocket
  })

  function getListenerCalls(event: string) {
    return addEventListenerFn.mock.calls
      .filter(([evt]: [string]) => evt === event)
      .map(([, listener]: [string, EventListener]) => listener)
  }

  function triggerOpen() {
    mockWs.readyState = 1
    getListenerCalls('open').forEach(l => l(new Event('open')))
  }

  function getLastListModelsId() {
    for (let i = sentData.length - 1; i >= 0; i--) {
      try {
        const parsed = JSON.parse(sentData[i])
        if (parsed.type === 'list_models') return parsed.id
      } catch {}
    }
    return null
  }

  function triggerMessage(data: object) {
    getListenerCalls('message').forEach(l => {
      l({ data: JSON.stringify(data) } as MessageEvent)
    })
  }

  it('should register and trigger listener via on/emit', async () => {
    const conn = getConnection()
    const handler = vi.fn()
    conn.on('models_updated', handler)

    const models: Model[] = [{ id: 'gpt-4', name: 'GPT-4', provider: 'openai' }]

    // Trigger ensureOpen by calling request
    conn.request({ type: 'ping', id: 'test-1' }).catch(() => {})
    triggerOpen()

    await new Promise(r => setTimeout(r, 10))

    // Reply to the auto-fetch list_models
    const autoFetchId = getLastListModelsId()
    expect(autoFetchId).toBeTruthy()
    triggerMessage({ type: 'models_list', id: autoFetchId, models })

    await new Promise(r => setTimeout(r, 10))
    expect(handler).toHaveBeenCalledWith(models)
  })

  it('should not trigger listener after off', async () => {
    const conn = getConnection()
    const handler = vi.fn()
    conn.on('models_updated', handler)
    conn.off('models_updated', handler)

    conn.request({ type: 'ping', id: 'test-1' }).catch(() => {})
    triggerOpen()

    await new Promise(r => setTimeout(r, 10))

    const autoFetchId = getLastListModelsId()
    triggerMessage({ type: 'models_list', id: autoFetchId, models: [] })

    expect(handler).not.toHaveBeenCalled()
  })

  it('should support multiple listeners on same event', async () => {
    const conn = getConnection()
    const handler1 = vi.fn()
    const handler2 = vi.fn()
    conn.on('models_updated', handler1)
    conn.on('models_updated', handler2)

    conn.request({ type: 'ping', id: 'test-1' }).catch(() => {})
    triggerOpen()

    await new Promise(r => setTimeout(r, 10))

    const autoFetchId = getLastListModelsId()
    const models: Model[] = [{ id: 'a', name: 'A', provider: 'p' }]
    triggerMessage({ type: 'models_list', id: autoFetchId, models })

    await new Promise(r => setTimeout(r, 10))
    expect(handler1).toHaveBeenCalledWith(models)
    expect(handler2).toHaveBeenCalledWith(models)
  })

  it('should emit connection_changed on open', async () => {
    const conn = getConnection()
    const handler = vi.fn()
    conn.on('connection_changed', handler)

    conn.request({ type: 'ping', id: 'test-1' }).catch(() => {})
    triggerOpen()

    expect(handler).toHaveBeenCalledWith('open')
  })

  it('should emit connection_changed on close', async () => {
    const conn = getConnection()
    const handler = vi.fn()
    conn.on('connection_changed', handler)

    conn.request({ type: 'ping', id: 'test-1' }).catch(() => {})
    triggerOpen()

    await new Promise(r => setTimeout(r, 0))

    const autoFetchId = getLastListModelsId()
    triggerMessage({ type: 'models_list', id: autoFetchId, models: [] })

    getListenerCalls('close').forEach(l => l(new CloseEvent('close')))

    expect(handler).toHaveBeenCalledWith('closed')
  })

  it('should auto-fetch models on open', async () => {
    const conn = getConnection()
    conn.on('models_updated', vi.fn())

    conn.request({ type: 'ping', id: 'test-1' }).catch(() => {})
    triggerOpen()

    await new Promise(r => setTimeout(r, 0))

    const autoFetchId = getLastListModelsId()
    expect(autoFetchId).toBeTruthy()
    const sent = JSON.parse(sentData.find(d => { try { return JSON.parse(d).type === 'list_models' } catch {} })!)
    expect(sent.type).toBe('list_models')
    expect(sent.id).toBeTruthy()
  })

  it('should not throw when emitting event with no listeners', async () => {
    const conn = getConnection()

    conn.request({ type: 'ping', id: 'test-1' }).catch(() => {})
    triggerOpen()

    await new Promise(r => setTimeout(r, 0))

    const autoFetchId = getLastListModelsId()
    expect(() => {
      triggerMessage({ type: 'models_list', id: autoFetchId, models: [] })
    }).not.toThrow()
  })

  it('should off specific listener without affecting others', async () => {
    const conn = getConnection()
    const handler1 = vi.fn()
    const handler2 = vi.fn()
    conn.on('connection_changed', handler1)
    conn.on('connection_changed', handler2)
    conn.off('connection_changed', handler1)

    conn.request({ type: 'ping', id: 'test-1' }).catch(() => {})
    triggerOpen()

    expect(handler1).not.toHaveBeenCalled()
    expect(handler2).toHaveBeenCalledWith('open')
  })
})
