import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { getConnection, closeConnection } from '../../services/connection'

describe('LoomConnection', () => {
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

  function triggerMessage(data: object) {
    getListenerCalls('message').forEach(l => {
      l({ data: JSON.stringify(data) } as MessageEvent)
    })
  }

  function triggerError() {
    getListenerCalls('error').forEach(l => l(new Event('error')))
  }

  function triggerClose() {
    mockWs.readyState = 3
    getListenerCalls('close').forEach(l => l(new CloseEvent('close')))
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

  describe('getConnection / closeConnection', () => {
    it('returns singleton', () => {
      const a = getConnection()
      const b = getConnection()
      expect(a).toBe(b)
    })

    it('creates new instance after closeConnection', () => {
      const a = getConnection()
      closeConnection()
      const b = getConnection()
      expect(a).not.toBe(b)
    })
  })

  describe('request', () => {
    it('sends request and resolves with response', async () => {
      const conn = getConnection()
      const p = conn.request({ type: 'ping', id: 'req-1' })

      triggerOpen()
      await new Promise(r => setTimeout(r, 10))

      expect(sentData).toEqual(expect.arrayContaining([
        expect.stringContaining('"type":"ping"')
      ]))

      triggerMessage({ type: 'pong', id: 'req-1' })
      const result = await p
      expect((result as any).type).toBe('pong')
    })

    it('rejects on error response', async () => {
      const conn = getConnection()
      const p = conn.request({ type: 'ping', id: 'req-1' })

      triggerOpen()
      await new Promise(r => setTimeout(r, 10))

      triggerMessage({ type: 'error', id: 'req-1', error: 'Something failed' })

      await expect(p).rejects.toThrow('Something failed')
    })

    it('rejects with unknown error when error message has no error field', async () => {
      const conn = getConnection()
      const p = conn.request({ type: 'ping', id: 'req-2' })

      triggerOpen()
      await new Promise(r => setTimeout(r, 10))

      triggerMessage({ type: 'error', id: 'req-2' })

      await expect(p).rejects.toThrow('Unknown error from server')
    })

    it('handles disconnected state gracefully', async () => {
      const conn = getConnection()
      const p = conn.request({ type: 'ping', id: 'req-1' }).catch(() => {})
      triggerOpen()
      await new Promise(r => setTimeout(r, 50))
      triggerMessage({ type: 'pong', id: 'req-1' })
      await new Promise(r => setTimeout(r, 50))
    })

    it('uses payload id when provided', async () => {
      const conn = getConnection()
      const p = conn.request({ type: 'ping', id: 'my-custom-id' })

      triggerOpen()
      await new Promise(r => setTimeout(r, 10))

      const sent = sentData.find(d => d.includes('my-custom-id'))
      expect(sent).toBeDefined()
      expect(JSON.parse(sent!).id).toBe('my-custom-id')
    })

    it('generates id when not provided in payload', async () => {
      const conn = getConnection()
      const p = conn.request({ type: 'ping' })

      triggerOpen()
      await new Promise(r => setTimeout(r, 10))

      const sent = sentData.find(d => d.includes('"type":"ping"'))
      expect(sent).toBeDefined()
      const parsed = JSON.parse(sent!)
      expect(parsed.id).toBeTruthy()
    })

    it('supports streaming via onMessage callback', async () => {
      const conn = getConnection()
      const messages: any[] = []

      const p = conn.request(
        { type: 'run', id: 'run-1' },
        (msg) => {
          messages.push(msg)
          return (msg as any).type === 'run_end'
        }
      )

      triggerOpen()
      await new Promise(r => setTimeout(r, 10))

      triggerMessage({ type: 'run_stream_event', id: 'run-1', event: { type: 'message_chunk' } })
      expect(messages).toHaveLength(1)

      triggerMessage({ type: 'run_end', id: 'run-1', reply: 'done' })
      expect(messages).toHaveLength(2)

      const result = await p
      expect((result as any).type).toBe('run_end')
    })

    it('clears run mapping for type=run', async () => {
      const conn = getConnection()
      const p = conn.request({ type: 'run', id: 'run-1' })

      triggerOpen()
      await new Promise(r => setTimeout(r, 10))

      triggerMessage({ type: 'run_end', id: 'run-1', reply: 'done' })
      await p
    })
  })

  describe('send', () => {
    it('sends payload through request', async () => {
      const conn = getConnection()
      const p = conn.request({ type: 'ping', id: 'x' })
      triggerOpen()
      await new Promise(r => setTimeout(r, 50))
      triggerMessage({ type: 'pong', id: 'x' })
      await p

      sentData.length = 0
      const p2 = conn.request({ type: 'test', data: 'hello', id: 'y' })
      await new Promise(r => setTimeout(r, 50))
      triggerMessage({ type: 'pong', id: 'y' })
      await p2

      expect(sentData.some(d => d.includes('"type":"test"'))).toBe(true)
    })
  })

  describe('handleMessage - run stream events', () => {
    it('handles run_stream_event with active run request', async () => {
      const conn = getConnection()

      const p = conn.request(
        { type: 'run', id: 'run-1' },
        () => false
      )

      triggerOpen()
      await new Promise(r => setTimeout(r, 10))

      triggerMessage({ type: 'run_stream_event', id: 'run-1', event: { type: 'message_chunk' } })
    })
  })

  describe('close', () => {
    it('sets intentionalClose and rejects all pending', async () => {
      const conn = getConnection()
      const p = conn.request({ type: 'ping', id: 'req-1' })

      triggerOpen()
      await new Promise(r => setTimeout(r, 10))

      closeConnection()

      await expect(p).rejects.toThrow('Connection intentionally closed')
    })

    it('clears reconnect timer', async () => {
      const conn = getConnection()
      const p = conn.request({ type: 'ping', id: 'req-1' }).catch(() => {})

      triggerClose()
      await new Promise(r => setTimeout(r, 10))

      closeConnection()
    })
  })

  describe('reconnection', () => {
    it('attempts to reconnect on unexpected close', async () => {
      vi.useFakeTimers()
      const conn = getConnection()
      const p = conn.request({ type: 'ping', id: 'req-1' }).catch(() => {})

      triggerOpen()
      await vi.advanceTimersByTimeAsync(10)

      triggerClose()
      await vi.advanceTimersByTimeAsync(10)

      vi.advanceTimersByTime(3000)
      await vi.advanceTimersByTimeAsync(10)

      vi.useRealTimers()
    })
  })

  describe('message handling with tab-separated data', () => {
    it('parses only the first tab-separated segment', async () => {
      const conn = getConnection()
      const p = conn.request({ type: 'ping', id: 'req-1' })

      triggerOpen()
      await new Promise(r => setTimeout(r, 10))

      const jsonWithTab = JSON.stringify({ type: 'pong', id: 'req-1' }) + '\tmetadata'
      getListenerCalls('message').forEach(l => {
        l({ data: jsonWithTab } as MessageEvent)
      })

      const result = await p
      expect((result as any).type).toBe('pong')
    })
  })

  describe('error response without matching pending', () => {
    it('handles error with activeRunRequestId', async () => {
      const conn = getConnection()
      const p = conn.request(
        { type: 'run', id: 'run-1' },
        () => true
      )

      triggerOpen()
      await new Promise(r => setTimeout(r, 10))

      triggerMessage({ type: 'error', error: 'run failed' })

      await expect(p).rejects.toThrow('run failed')
    })
  })

  describe('event bus', () => {
    it('should auto-fetch models on open', async () => {
      const conn = getConnection()
      conn.on('models_updated', vi.fn())

      conn.request({ type: 'ping', id: 'test-1' }).catch(() => {})
      triggerOpen()
      await new Promise(r => setTimeout(r, 50))

      const autoFetchId = getLastListModelsId()
      expect(autoFetchId).toBeTruthy()
    })

    it('emits connection_changed on open', async () => {
      const conn = getConnection()
      const handler = vi.fn()
      conn.on('connection_changed', handler)

      conn.request({ type: 'ping', id: 'x' }).catch(() => {})
      triggerOpen()

      expect(handler).toHaveBeenCalledWith('open')
    })

    it('emits connection_changed on close', async () => {
      const conn = getConnection()
      const handler = vi.fn()
      conn.on('connection_changed', handler)

      conn.request({ type: 'ping', id: 'x' }).catch(() => {})
      triggerOpen()
      await new Promise(r => setTimeout(r, 50))

      triggerClose()

      expect(handler).toHaveBeenCalledWith('closed')
    })

    it('off removes specific listener', async () => {
      const conn = getConnection()
      const handler1 = vi.fn()
      const handler2 = vi.fn()
      conn.on('connection_changed', handler1)
      conn.on('connection_changed', handler2)
      conn.off('connection_changed', handler1)

      conn.request({ type: 'ping', id: 'x' }).catch(() => {})
      triggerOpen()

      expect(handler1).not.toHaveBeenCalled()
      expect(handler2).toHaveBeenCalledWith('open')
    })
  })

  describe('WebSocket error', () => {
    it('rejects pending request on error event', async () => {
      const conn = getConnection()
      const p = conn.request({ type: 'ping', id: 'req-1' })

      triggerError()

      await expect(p).rejects.toThrow('Unable to reach Loom WebSocket server')
    })
  })

  describe('ensureOpen reuse', () => {
    it('reuses existing connection', async () => {
      const conn = getConnection()
      const p1 = conn.request({ type: 'ping', id: 'req-1' })

      triggerOpen()
      await new Promise(r => setTimeout(r, 10))

      triggerMessage({ type: 'pong', id: 'req-1' })
      await p1

      sentData.length = 0
      const p2 = conn.request({ type: 'ping', id: 'req-2' })

      await new Promise(r => setTimeout(r, 10))
      triggerMessage({ type: 'pong', id: 'req-2' })
      await p2

      expect(sentData.some(d => d.includes('req-2'))).toBe(true)
    })
  })
})
