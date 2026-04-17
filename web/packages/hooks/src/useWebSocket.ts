import { useCallback, useEffect, useRef, useState } from 'react'

export type WebSocketStatus = 'connecting' | 'connected' | 'disconnected' | 'error'

export interface UseWebSocketOptions {
  url: string
  onMessage?: (event: MessageEvent) => void
  onError?: (error: Event) => void
  onOpen?: () => void
  onClose?: () => void
  reconnectAttempts?: number
  reconnectInterval?: number
}

export interface UseWebSocketReturn {
  status: WebSocketStatus
  error: string | null
  connect: () => void
  disconnect: () => void
  send: (data: string | object) => void
}

export function useWebSocket({
  url,
  onMessage,
  onError,
  onOpen,
  onClose,
  reconnectAttempts = 5,
  reconnectInterval = 3000,
}: UseWebSocketOptions): UseWebSocketReturn {
  const [status, setStatus] = useState<WebSocketStatus>('disconnected')
  const [error, setError] = useState<string | null>(null)
  
  const socketRef = useRef<WebSocket | null>(null)
  const connectRef = useRef<() => void>(() => {})
  const reconnectCountRef = useRef(0)
  const reconnectTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const connect = useCallback(() => {
    if (socketRef.current?.readyState === WebSocket.OPEN) {
      return
    }

    setStatus('connecting')
    setError(null)

    try {
      const socket = new WebSocket(url)
      socketRef.current = socket

      socket.onopen = () => {
        setStatus('connected')
        setError(null)
        reconnectCountRef.current = 0
        onOpen?.()
      }

      socket.onmessage = (event) => {
        onMessage?.(event)
      }

      socket.onerror = (error) => {
        setStatus('error')
        setError('WebSocket connection error')
        onError?.(error)
      }

      socket.onclose = () => {
        setStatus('disconnected')
        onClose?.()

        // Auto reconnect
        if (reconnectCountRef.current < reconnectAttempts) {
          reconnectCountRef.current++
          reconnectTimeoutRef.current = setTimeout(() => {
            connectRef.current()
          }, reconnectInterval)
        }
      }
    } catch (err) {
      setStatus('error')
      setError(err instanceof Error ? err.message : 'Failed to connect')
    }
  }, [url, onMessage, onError, onOpen, onClose, reconnectAttempts, reconnectInterval])

  useEffect(() => {
    connectRef.current = connect
  }, [connect])

  const disconnect = useCallback(() => {
    if (reconnectTimeoutRef.current) {
      clearTimeout(reconnectTimeoutRef.current)
    }
    
    if (socketRef.current) {
      socketRef.current.close()
      socketRef.current = null
    }
    
    setStatus('disconnected')
    setError(null)
  }, [])

  const send = useCallback((data: string | object) => {
    if (socketRef.current?.readyState === WebSocket.OPEN) {
      const message = typeof data === 'string' ? data : JSON.stringify(data)
      socketRef.current.send(message)
    } else {
      console.warn('WebSocket is not connected')
    }
  }, [])

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current)
      }
      if (socketRef.current) {
        socketRef.current.close()
      }
    }
  }, [])

  return {
    status,
    error,
    connect,
    disconnect,
    send,
  }
}
