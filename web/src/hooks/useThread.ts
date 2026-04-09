import { useState, useCallback } from 'react'

const THREAD_STORAGE_KEY = 'loom-web-thread-id'

export function useThread() {
  const [threadId, setThreadIdState] = useState<string>(() => {
    const existing = window.localStorage.getItem(THREAD_STORAGE_KEY)
    if (existing) {
      return existing
    }

    const nextThreadId = crypto.randomUUID()
    window.localStorage.setItem(THREAD_STORAGE_KEY, nextThreadId)
    return nextThreadId
  })

  const setThreadId = useCallback((id: string) => {
    window.localStorage.setItem(THREAD_STORAGE_KEY, id)
    setThreadIdState(id)
  }, [])

  const resetThread = useCallback((): string => {
    const newThreadId = crypto.randomUUID()
    window.localStorage.setItem(THREAD_STORAGE_KEY, newThreadId)
    setThreadIdState(newThreadId)
    return newThreadId
  }, [])

  return {
    threadId,
    setThreadId,
    resetThread,
  }
}