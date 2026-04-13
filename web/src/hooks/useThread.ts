import { useState, useCallback, useEffect, useRef } from 'react'
import { addThread } from '../services/workspace'

const THREAD_STORAGE_KEY = 'loom-web-thread-id'

export function useThread(workspaceId?: string) {
  const [threadId, setThreadIdState] = useState<string>(() => {
    const existing = window.localStorage.getItem(THREAD_STORAGE_KEY)
    if (existing) {
      return existing
    }

    const nextThreadId = crypto.randomUUID()
    window.localStorage.setItem(THREAD_STORAGE_KEY, nextThreadId)
    return nextThreadId
  })

  const registeredRef = useRef<string>('')

  const setThreadId = useCallback((id: string) => {
    window.localStorage.setItem(THREAD_STORAGE_KEY, id)
    setThreadIdState(id)
  }, [])

  useEffect(() => {
    if (workspaceId && threadId) {
      const key = `${workspaceId}:${threadId}`
      if (registeredRef.current !== key) {
        registeredRef.current = key
        addThread(workspaceId, threadId).catch(error => {
          console.warn('Failed to add thread to workspace:', error)
        })
      }
    }
  }, [workspaceId, threadId])

  const resetThread = useCallback((): string => {
    const newThreadId = crypto.randomUUID()
    window.localStorage.setItem(THREAD_STORAGE_KEY, newThreadId)
    setThreadIdState(newThreadId)

    if (workspaceId) {
      const key = `${workspaceId}:${newThreadId}`
      registeredRef.current = key
      addThread(workspaceId, newThreadId).catch(error => {
        console.warn('Failed to add thread to workspace:', error)
      })
    }

    return newThreadId
  }, [workspaceId])

  return {
    threadId,
    setThreadId,
    resetThread,
  }
}