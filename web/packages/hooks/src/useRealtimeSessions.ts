import { useState, useEffect, useCallback, useRef } from 'react'
import { getConnection } from '@graphweave/ws-client'
import { listSessions } from '@graphweave/service-workspace'
import type { Session } from '@graphweave/types'
import type { SessionInWorkspace } from '@graphweave/protocol'

export type SessionEvent = {
  type: 'created' | 'updated' | 'deleted'
  workspaceId: string
  sessionId: string
  sessionName?: string
  timestamp: number
}

export interface UseRealtimeSessionsReturn {
  sessions: Session[]
  loading: boolean
  error: string | null
  refresh: () => Promise<void>
}

/**
 * Real-time session list hook with WebSocket push notifications
 * 
 * This hook subscribes to server-side session events and automatically
 * updates the session list when new sessions are created, updated, or deleted.
 * 
 * @param workspaceId - The workspace ID to monitor (undefined = no workspace)
 * @returns Session list, loading state, error, and refresh function
 */
export function useRealtimeSessions(workspaceId?: string): UseRealtimeSessionsReturn {
  const [sessions, setSessions] = useState<Session[]>([])
  const [loading, setLoading] = useState<boolean>(true)
  const [error, setError] = useState<string | null>(null)
  
  // Use ref to track workspaceId changes without triggering re-subscription
  const workspaceIdRef = useRef(workspaceId)
  
  // Convert SessionInWorkspace to Session format
  const convertToSession = useCallback((data: SessionInWorkspace): Session => {
    return {
      id: data.thread_id,
      title: 'Untitled Session',
      createdAt: new Date(data.created_at_ms).toISOString(),
      updatedAt: new Date(data.created_at_ms).toISOString(),
      lastMessage: '',
      messageCount: 0,
      agent: 'default',
      model: 'default',
      workspace: workspaceId,
      isPinned: false,
      isArchived: false,
      status: 'active',
    }
  }, [workspaceId])

  // Load sessions from API
  const loadSessions = useCallback(async () => {
    if (!workspaceId) {
      setSessions([])
      setLoading(false)
      return
    }

    setLoading(true)
    setError(null)
    
    try {
      const data = await listSessions(workspaceId)
      const convertedSessions = data.map(convertToSession)
      setSessions(convertedSessions)
    } catch (err) {
      console.error('[useRealtimeSessions] Failed to load sessions:', err)
      setError(err instanceof Error ? err.message : 'Failed to load sessions')
    } finally {
      setLoading(false)
    }
  }, [workspaceId, convertToSession])

  // Subscribe to server push events
  useEffect(() => {
    // Only subscribe if we have a workspace AND it's a new workspace (not just a re-render)
    if (!workspaceId || workspaceIdRef.current === workspaceId) {
      // If workspace changed to undefined, clear sessions
      if (!workspaceId && workspaceIdRef.current) {
        setSessions([])
        setLoading(false)
      }
      workspaceIdRef.current = workspaceId
      return
    }
    
    workspaceIdRef.current = workspaceId
    
    const conn = getConnection()

    // Handler for new session created
    const handleSessionCreated = (event: {
      workspaceId: string
      sessionId: string
      sessionName?: string
      createdAt: string
    }) => {
      if (event.workspaceId !== workspaceId) return

      console.log('[useRealtimeSessions] Session created:', event.sessionId)
      
      // Add new session to the list
      const newSession: Session = {
        id: event.sessionId,
        title: event.sessionName || 'New Session',
        createdAt: event.createdAt,
        updatedAt: event.createdAt,
        lastMessage: '',
        messageCount: 0,
        agent: 'default',
        model: 'default',
        workspace: workspaceId,
        isPinned: false,
        isArchived: false,
        status: 'active',
      }

      setSessions(prev => {
        // Avoid duplicates
        if (prev.some(s => s.id === event.sessionId)) {
          return prev
        }
        return [newSession, ...prev]
      })
    }

    // Handler for session updated
    const handleSessionUpdated = (event: {
      workspaceId: string
      sessionId: string
      sessionName?: string
      updatedAt: string
    }) => {
      if (event.workspaceId !== workspaceId) return

      console.log('[useRealtimeSessions] Session updated:', event.sessionId)
      
      setSessions(prev =>
        prev.map(session =>
          session.id === event.sessionId
            ? {
                ...session,
                title: event.sessionName || session.title,
                updatedAt: event.updatedAt,
              }
            : session
        )
      )
    }

    // Handler for session deleted
    const handleSessionDeleted = (event: {
      workspaceId: string
      sessionId: string
    }) => {
      if (event.workspaceId !== workspaceId) return

      console.log('[useRealtimeSessions] Session deleted:', event.sessionId)
      
      setSessions(prev => prev.filter(session => session.id !== event.sessionId))
    }

    // Register event listeners
    conn.on('session_created', handleSessionCreated)
    conn.on('session_updated', handleSessionUpdated)
    conn.on('session_deleted', handleSessionDeleted)

    // Load initial sessions
    loadSessions()

    // Cleanup function
    return () => {
      conn.off('session_created', handleSessionCreated)
      conn.off('session_updated', handleSessionUpdated)
      conn.off('session_deleted', handleSessionDeleted)
    }
  }, [workspaceId, loadSessions])

  return {
    sessions,
    loading,
    error,
    refresh: loadSessions,
  }
}