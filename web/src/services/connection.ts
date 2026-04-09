import type { LoomServerMessage } from '../types/protocol/loom'
import { isError } from '../types/protocol/loom'

function getEnvValue(name: string) {
  return (import.meta.env as Record<string, string | undefined>)[name]?.trim()
}

function getLoomWsUrl() {
  return getEnvValue('VITE_LOOM_WS_URL') || 'ws://127.0.0.1:8080'
}

export type MessageHandler = (msg: LoomServerMessage) => boolean

type PendingEntry = {
  resolve: (msg: LoomServerMessage) => void
  reject: (error: Error) => void
  onMessage?: MessageHandler
}

class LoomConnection {
  private ws: WebSocket | null = null
  private pending = new Map<string, PendingEntry>()
  private url: string
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private intentionalClose = false
  private opening: Promise<void> | null = null

  constructor() {
    this.url = getLoomWsUrl()
  }

  private ensureOpen(): Promise<void> {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      return Promise.resolve()
    }

    if (this.opening) {
      return this.opening
    }

    this.opening = new Promise<void>((resolve, reject) => {
      const ws = new WebSocket(this.url)

      ws.addEventListener('open', () => {
        this.opening = null
        resolve()
      })

      ws.addEventListener('message', (event: MessageEvent<string>) => {
        if (typeof event.data !== 'string') return
        let msg: LoomServerMessage
        try {
          const jsonStr = event.data.split('\t')[0]
          msg = JSON.parse(jsonStr) as LoomServerMessage
        } catch {
          return
        }
        this.handleMessage(msg)
      })

      ws.addEventListener('error', () => {
        this.opening = null
        reject(new Error(`Unable to reach Loom WebSocket server at ${this.url}. Start it with \`loom serve\`.`))
      })

      ws.addEventListener('close', () => {
        if (this.ws === ws) {
          this.ws = null
          this.opening = null
        }
        if (!this.intentionalClose) {
          this.rejectAllPending(new Error('WebSocket connection closed.'))
          this.scheduleReconnect()
        }
      })

      this.ws = ws
      this.intentionalClose = false
    })

    return this.opening
  }

  private handleMessage(msg: LoomServerMessage) {
    const id = (msg as Record<string, unknown>).id as string | undefined

    if (isError(msg)) {
      if (id && this.pending.has(id)) {
        const entry = this.pending.get(id)!
        this.pending.delete(id)
        entry.reject(new Error(msg.error || 'Unknown error from server.'))
      }
      return
    }

    if (id && this.pending.has(id)) {
      const entry = this.pending.get(id)!
      if (entry.onMessage) {
        const done = entry.onMessage(msg)
        if (done) {
          this.pending.delete(id)
          entry.resolve(msg)
        }
      } else {
        this.pending.delete(id)
        entry.resolve(msg)
      }
    }
  }

  private rejectAllPending(error: Error) {
    for (const [id, entry] of this.pending) {
      this.pending.delete(id)
      entry.reject(error)
    }
  }

  private scheduleReconnect() {
    if (this.reconnectTimer) return
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null
      this.ensureOpen().catch(() => {
        this.scheduleReconnect()
      })
    }, 3000)
  }

  async request(
    payload: object,
    onMessage?: MessageHandler,
  ): Promise<LoomServerMessage> {
    await this.ensureOpen()

    const id = ((payload as Record<string, unknown>).id as string) || crypto.randomUUID()
    const request = { ...payload, id }

    return new Promise<LoomServerMessage>((resolve, reject) => {
    this.pending.set(id, { resolve, reject, onMessage })

    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
        this.pending.delete(id)
        reject(new Error('WebSocket is not connected.'))
        return
      }

      this.ws.send(JSON.stringify(request))
    })
  }

  async send(payload: object): Promise<void> {
    await this.ensureOpen()
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      throw new Error('WebSocket is not connected.')
    }
    this.ws.send(JSON.stringify(payload))
  }

  close() {
    this.intentionalClose = true
    this.opening = null
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    if (this.ws) {
      this.ws.close()
      this.ws = null
    }
    this.rejectAllPending(new Error('Connection intentionally closed.'))
  }
}

let _instance: LoomConnection | null = null

export function getConnection(): LoomConnection {
  if (!_instance) {
    _instance = new LoomConnection()
  }
  return _instance
}

export function closeConnection(): void {
  if (_instance) {
    _instance.close()
    _instance = null
  }
}
