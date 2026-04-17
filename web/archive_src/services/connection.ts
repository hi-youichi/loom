import type { LoomServerMessage, CancelRunRequest, CancelRunResponse } from '../types/protocol/loom'
import {
  isError,
  isSessionCreatedEvent,
  isSessionUpdatedEvent,
  isSessionDeletedEvent,
} from '../types/protocol/loom'

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

export type LoomEventType =
  | 'models_updated'
  | 'connection_changed'
  | 'session_created'
  | 'session_updated'
  | 'session_deleted'

export type LoomEventMap = {
  models_updated: Model[]
  connection_changed: 'open' | 'closed'
  session_created: {
    workspaceId: string
    sessionId: string
    sessionName?: string
    createdAt: string
  }
  session_updated: {
    workspaceId: string
    sessionId: string
    sessionName?: string
    updatedAt: string
  }
  session_deleted: {
    workspaceId: string
    sessionId: string
  }
}

export interface Model {
  id: string
  name: string
  provider: string
  family?: string
  capabilities?: string[]
}

type Listener<T> = (data: T) => void

class LoomConnection {
  private ws: WebSocket | null = null
  private pending = new Map<string, PendingEntry>()
  private url: string
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private intentionalClose = false
  private opening: Promise<void> | null = null
  private activeRunRequestId: string | null = null
  private activeRunId: string | null = null
  private listeners = new Map<string, Set<Listener<unknown>>>()

  constructor() {
    this.url = getLoomWsUrl()
  }

  on<K extends LoomEventType>(event: K, listener: Listener<LoomEventMap[K]>): void {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set())
    }
    this.listeners.get(event)!.add(listener as Listener<unknown>)
  }

  off<K extends LoomEventType>(event: K, listener: Listener<LoomEventMap[K]>): void {
    this.listeners.get(event)?.delete(listener as Listener<unknown>)
  }

  private emit<K extends LoomEventType>(event: K, data: LoomEventMap[K]): void {
    this.listeners.get(event)?.forEach(fn => fn(data))
  }

  private notifyConnectionChanged(state: 'open' | 'closed') {
    this.emit('connection_changed', state)
  }

  private async fetchAndEmitModels() {
    try {
      const id = crypto.randomUUID()
      const response = await this.request({ type: 'list_models', id }) as { models: Model[] }
      this.emit('models_updated', response.models || [])
    } catch (e) {
      console.warn('Auto-fetch models failed:', e)
    }
  }

  private ensureOpen(): Promise<void> {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) return Promise.resolve()
    if (this.opening) return this.opening

    this.opening = new Promise<void>((resolve, reject) => {
      const ws = new WebSocket(this.url)

      ws.addEventListener('open', () => {
        this.opening = null
        this.notifyConnectionChanged('open')
        this.fetchAndEmitModels()
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
          this.notifyConnectionChanged('closed')
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
    const type = (msg as Record<string, unknown>).type as string | undefined

    if (isError(msg)) {
      if (id && this.pending.has(id)) {
        const entry = this.pending.get(id)!
        this.pending.delete(id)
        entry.reject(new Error(msg.error || 'Unknown error from server.'))
      } else if (this.activeRunRequestId) {
        const reqId = this.activeRunRequestId
        const entry = this.pending.get(reqId)
        if (entry) {
          this.pending.delete(reqId)
          this.clearRunMapping()
          entry.reject(new Error(msg.error || 'Unknown error from server.'))
        }
      }
      return
    }

    if (id && this.pending.has(id)) {
      const entry = this.pending.get(id)!
      if (entry.onMessage) {
        const done = entry.onMessage(msg)
        if (done) {
          this.pending.delete(id)
          this.clearRunMapping()
          entry.resolve(msg)
        }
      } else {
        this.pending.delete(id)
        this.clearRunMapping()
        entry.resolve(msg)
      }
      return
    }

    if ((type === 'run_stream_event' || type === 'run_end') && id) {
      if (!this.activeRunId && this.activeRunRequestId) {
        this.activeRunId = id
        const reqId = this.activeRunRequestId
        const entry = this.pending.get(reqId)
        if (entry) {
          this.pending.set(id, entry)
          this.pending.delete(reqId)
        }
      }
      if (this.activeRunId === id && this.pending.has(id)) {
        const entry = this.pending.get(id)!
        if (entry.onMessage) {
          const done = entry.onMessage(msg)
          if (done) {
            this.pending.delete(id)
            this.clearRunMapping()
            entry.resolve(msg)
          }
        } else {
          this.pending.delete(id)
          this.clearRunMapping()
          entry.resolve(msg)
        }
      }
    }

    // Handle session events (server push notifications)
    if (isSessionCreatedEvent(msg)) {
      this.emit('session_created', {
        workspaceId: msg.workspace_id,
        sessionId: msg.session_id,
        sessionName: msg.session_name,
        createdAt: msg.created_at,
      })
      return
    }

    if (isSessionUpdatedEvent(msg)) {
      this.emit('session_updated', {
        workspaceId: msg.workspace_id,
        sessionId: msg.session_id,
        sessionName: msg.session_name,
        updatedAt: msg.updated_at,
      })
      return
    }

    if (isSessionDeletedEvent(msg)) {
      this.emit('session_deleted', {
        workspaceId: msg.workspace_id,
        sessionId: msg.session_id,
      })
      return
    }
  }

  private clearRunMapping() {
    this.activeRunRequestId = null
    this.activeRunId = null
  }

  private rejectAllPending(error: Error) {
    this.clearRunMapping()
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

    if ((payload as Record<string, unknown>).type === 'run') {
      this.activeRunRequestId = id
    }

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

  async cancelRun(runId: string): Promise<void> {
    const requestId = crypto.randomUUID()
    const request: CancelRunRequest = {
      type: 'cancel_run',
      id: requestId,
      run_id: runId
    }

    return new Promise<void>((resolve, reject) => {
      const onMessage = (msg: LoomServerMessage): boolean => {
        const cancelAck = msg as CancelRunResponse
        if (cancelAck.type === 'cancel_run' && cancelAck.id === requestId) {
          resolve()
          return true
        }
        return false
      }

      this.pending.set(requestId, { resolve: () => {}, reject, onMessage })

      if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
        this.pending.delete(requestId)
        reject(new Error('WebSocket is not connected.'))
        return
      }

      this.ws.send(JSON.stringify(request))
    })
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
