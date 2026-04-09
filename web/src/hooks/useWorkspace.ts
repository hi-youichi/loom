import { useState, useCallback, useEffect, useRef } from 'react'
import type { WorkspaceMeta, ThreadInWorkspace } from '../types/protocol/loom'
import * as wsApi from '../services/workspace'

const STORAGE_KEY = 'loom-active-workspace-id'

function getErrorMessage(e: unknown, fallback: string): string {
  return e instanceof Error ? e.message : fallback
}

export function useWorkspace() {
  const [workspaces, setWorkspaces] = useState<WorkspaceMeta[]>([])
  const [activeWorkspaceId, setActiveWorkspaceId] = useState<string | null>(() => {
    return localStorage.getItem(STORAGE_KEY) || null
  })
  const [threads, setThreads] = useState<ThreadInWorkspace[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const mountedRef = useRef(true)

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
    }
  }, [])

  useEffect(() => {
    if (activeWorkspaceId) {
      localStorage.setItem(STORAGE_KEY, activeWorkspaceId)
    } else {
      localStorage.removeItem(STORAGE_KEY)
    }
  }, [activeWorkspaceId])

  const loadWorkspaces = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const list = await wsApi.listWorkspaces()
      if (mountedRef.current) {
        setWorkspaces(list)
      }
    } catch (e: unknown) {
      if (mountedRef.current) {
        setError(getErrorMessage(e, 'Failed to load workspaces'))
      }
    } finally {
      if (mountedRef.current) {
        setLoading(false)
      }
    }
  }, [])

  const createWorkspace = useCallback(async (name?: string) => {
    setLoading(true)
    setError(null)
    try {
      const workspaceId = await wsApi.createWorkspace(name)
      if (mountedRef.current) {
        setActiveWorkspaceId(workspaceId)
        await loadWorkspaces()
      }
      return workspaceId
    } catch (e: unknown) {
      if (mountedRef.current) {
        setError(getErrorMessage(e, 'Failed to create workspace'))
      }
      return null
    } finally {
      if (mountedRef.current) {
        setLoading(false)
      }
    }
  }, [loadWorkspaces])

  const selectWorkspace = useCallback(async (id: string) => {
    setActiveWorkspaceId(id)
    setLoading(true)
    setError(null)
    try {
      const list = await wsApi.listThreads(id)
      if (mountedRef.current) {
        setThreads(list)
      }
    } catch (e: unknown) {
      if (mountedRef.current) {
        setError(getErrorMessage(e, 'Failed to load threads'))
      }
    } finally {
      if (mountedRef.current) {
        setLoading(false)
      }
    }
  }, [])

  const addThread = useCallback(async (threadId: string) => {
    if (!activeWorkspaceId) return
    try {
      await wsApi.addThread(activeWorkspaceId, threadId)
      if (mountedRef.current) {
        setThreads(prev => [...prev, { thread_id: threadId, created_at_ms: Date.now() }])
      }
    } catch (e: unknown) {
      if (mountedRef.current) {
        setError(getErrorMessage(e, 'Failed to add thread'))
      }
    }
  }, [activeWorkspaceId])

  const removeThread = useCallback(async (threadId: string) => {
    if (!activeWorkspaceId) return
    try {
      await wsApi.removeThread(activeWorkspaceId, threadId)
      if (mountedRef.current) {
        setThreads(prev => prev.filter(t => t.thread_id !== threadId))
      }
    } catch (e: unknown) {
      if (mountedRef.current) {
        setError(getErrorMessage(e, 'Failed to remove thread'))
      }
    }
  }, [activeWorkspaceId])

  const clearActiveWorkspace = useCallback(() => {
    setActiveWorkspaceId(null)
    setThreads([])
    localStorage.removeItem(STORAGE_KEY)
  }, [])

  return {
    workspaces,
    activeWorkspaceId,
    threads,
    loading,
    error,
    loadWorkspaces,
    createWorkspace,
    selectWorkspace,
    addThread,
    removeThread,
    clearActiveWorkspace,
  }
}