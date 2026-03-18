import { useState, useCallback } from 'react'

const THREAD_STORAGE_KEY = 'loom-web-thread-id'

/**
 * 管理线程ID的Hook
 * - 自动持久化到localStorage
 * - 支持重置线程
 */
export function useThread() {
  const [threadId, setThreadId] = useState<string>(() => {
    const existing = window.localStorage.getItem(THREAD_STORAGE_KEY)
    if (existing) {
      return existing
    }

    const nextThreadId = crypto.randomUUID()
    window.localStorage.setItem(THREAD_STORAGE_KEY, nextThreadId)
    return nextThreadId
  })

  const resetThread = useCallback(() => {
    const newThreadId = crypto.randomUUID()
    window.localStorage.setItem(THREAD_STORAGE_KEY, newThreadId)
    setThreadId(newThreadId)
  }, [])

  return {
    threadId,
    resetThread,
  }
}
