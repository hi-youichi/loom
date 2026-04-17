import { useState, useCallback, useEffect, useRef } from 'react'
import { addSession } from '@loom/service-workspace'

const SESSION_STORAGE_KEY = 'loom-web-session-id'

export function useSessionId(workspaceId?: string) {
  const [sessionId, setSessionIdState] = useState<string>(() => {
    const existing = window.localStorage.getItem(SESSION_STORAGE_KEY)
    if (existing) {
      return existing
    }

    const nextSessionId = crypto.randomUUID()
    window.localStorage.setItem(SESSION_STORAGE_KEY, nextSessionId)
    return nextSessionId
  })

  const registeredRef = useRef<string>('')

  const setSessionId = useCallback((id: string) => {
    window.localStorage.setItem(SESSION_STORAGE_KEY, id)
    setSessionIdState(id)
  }, [])

  useEffect(() => {
    if (workspaceId && sessionId) {
      const key = `${workspaceId}:${sessionId}`
      if (registeredRef.current !== key) {
        registeredRef.current = key
        addSession(workspaceId, sessionId).catch(error => {
          console.warn('Failed to add session to workspace:', error)
        })
      }
    }
  }, [workspaceId, sessionId])

  const resetSession = useCallback((): string => {
    const newSessionId = crypto.randomUUID()
    window.localStorage.setItem(SESSION_STORAGE_KEY, newSessionId)
    setSessionIdState(newSessionId)

    if (workspaceId) {
      const key = `${workspaceId}:${newSessionId}`
      registeredRef.current = key
      addSession(workspaceId, newSessionId).catch(error => {
        console.warn('Failed to add session to workspace:', error)
      })
    }

    return newSessionId
  }, [workspaceId])

  return {
    sessionId,
    setSessionId,
    resetSession,
  }
}
