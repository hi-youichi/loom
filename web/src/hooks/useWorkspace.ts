import { useState, useCallback, useEffect, useRef } from 'react'
import type { WorkspaceMeta, SessionInWorkspace } from '../types/protocol/loom'
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
  const [sessions, setSessions] = useState<SessionInWorkspace[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const mountedRef = useRef(true)

  useEffect(() => {
    return () => { mountedRef.current = false }
  }, [])

  const loadWorkspaces = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const wsList = await wsApi.listWorkspaces()
      if (mountedRef.current) setWorkspaces(wsList)
    } catch (e: unknown) {
      if (mountedRef.current) setError(getErrorMessage(e, 'Failed to load workspaces'))
    } finally {
      if (mountedRef.current) setLoading(false)
    }
  }, [])

  const createWorkspace = useCallback(async (name?: string): Promise<string> => {
    setLoading(true)
    try {
      const id = await wsApi.createWorkspace(name)
      if (mountedRef.current) {
        const wsList = await wsApi.listWorkspaces()
        setWorkspaces(wsList)
        setActiveWorkspaceId(id)
        localStorage.setItem(STORAGE_KEY, id)
      }
      return id
    } catch (e: unknown) {
      if (mountedRef.current) setError(getErrorMessage(e, 'Failed to create workspace'))
      throw e
    } finally {
      if (mountedRef.current) setLoading(false)
    }
  }, [])

  const selectWorkspace = useCallback(async (id: string) => {
    setActiveWorkspaceId(id)
    localStorage.setItem(STORAGE_KEY, id)
    setLoading(true)
    setError(null)
    try {
      const sessionList = await wsApi.listSessions(id)
      if (mountedRef.current) setSessions(sessionList)
    } catch (e: unknown) {
      if (mountedRef.current) setError(getErrorMessage(e, 'Failed to load workspace sessions'))
    } finally {
      if (mountedRef.current) setLoading(false)
    }
  }, [])

  const addSession = useCallback(async (sessionId: string) => {
    if (!activeWorkspaceId) return
    try {
      await wsApi.addSession(activeWorkspaceId, sessionId)
      if (mountedRef.current) {
        setSessions(prev => [...prev, { thread_id: sessionId, created_at_ms: Date.now() }])
      }
    } catch (e: unknown) {
      if (mountedRef.current) {
        setError(getErrorMessage(e, 'Failed to add session'))
      }
    }
  }, [activeWorkspaceId])

  const removeSession = useCallback(async (sessionId: string) => {
    if (!activeWorkspaceId) return
    try {
      await wsApi.removeSession(activeWorkspaceId, sessionId)
      if (mountedRef.current) {
        setSessions(prev => prev.filter(t => t.thread_id !== sessionId))
      }
    } catch (e: unknown) {
      if (mountedRef.current) {
        setError(getErrorMessage(e, 'Failed to remove session'))
      }
    }
  }, [activeWorkspaceId])

  const clearActiveWorkspace = useCallback(() => {
    setActiveWorkspaceId(null)
    setSessions([])
    localStorage.removeItem(STORAGE_KEY)
  }, [])

  return {
    workspaces,
    activeWorkspaceId,
    sessions,
    loading,
    error,
    loadWorkspaces,
    createWorkspace,
    selectWorkspace,
    addSession,
    removeSession,
    clearActiveWorkspace,
  }
}
