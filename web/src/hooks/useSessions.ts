/**
 * Custom hook for managing sessions
 */

import { useState, useEffect, useCallback, useRef } from 'react'
import { SessionService } from '../services/session'
import type { Session, SessionFilters, SessionListOptions } from '../types/session'

export function useSessions() {
  const [sessions, setSessions] = useState<Session[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const mountedRef = useRef(true)

  // Fetch sessions on mount
  const refreshSessions = useCallback(async () => {
    if (!mountedRef.current) return
    
    setLoading(true)
    setError(null)
    
    try {
      const data = await SessionService.listSessions()
      if (mountedRef.current) {
        setSessions(data)
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to load sessions'
      if (mountedRef.current) {
        setError(errorMessage)
      }
    } finally {
      if (mountedRef.current) {
        setLoading(false)
      }
    }
  }, [])

  // Create a new session
  const createSession = useCallback(async (data?: Partial<Session>) => {
    try {
      const newSession = await SessionService.createSession(data)
      if (mountedRef.current) {
        setSessions(prev => [newSession, ...prev])
      }
      return newSession
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to create session'
      setError(errorMessage)
      throw err
    }
  }, [])

  // Update a session
  const updateSession = useCallback(async (id: string, data: Partial<Session>) => {
    try {
      const updated = await SessionService.updateSession(id, data)
      if (updated && mountedRef.current) {
        setSessions(prev =>
          prev.map(session => session.id === id ? updated : session)
        )
      }
      return updated
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to update session'
      setError(errorMessage)
      throw err
    }
  }, [])

  // Delete a session
  const deleteSession = useCallback(async (id: string) => {
    try {
      const success = await SessionService.deleteSession(id)
      if (success && mountedRef.current) {
        setSessions(prev => prev.filter(session => session.id !== id))
      }
      return success
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to delete session'
      setError(errorMessage)
      throw err
    }
  }, [])

  // Toggle pin status
  const togglePin = useCallback(async (id: string) => {
    try {
      const updated = await SessionService.togglePinSession(id)
      if (updated && mountedRef.current) {
        setSessions(prev =>
          prev.map(session => session.id === id ? updated : session)
        )
      }
      return updated
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to toggle pin'
      setError(errorMessage)
      throw err
    }
  }, [])

  // Toggle archive status
  const toggleArchive = useCallback(async (id: string) => {
    try {
      const updated = await SessionService.toggleArchiveSession(id)
      if (updated && mountedRef.current) {
        setSessions(prev =>
          prev.map(session => session.id === id ? updated : session)
        )
      }
      return updated
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to toggle archive'
      setError(errorMessage)
      throw err
    }
  }, [])

  // Add message to session
  const addMessage = useCallback(async (sessionId: string, message: string, sender: 'user' | 'assistant') => {
    try {
      const updated = await SessionService.addMessage(sessionId, message, sender)
      if (updated && mountedRef.current) {
        setSessions(prev =>
          prev.map(session => session.id === sessionId ? updated : session)
        )
      }
      return updated
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to add message'
      setError(errorMessage)
      throw err
    }
  }, [])

  // Search sessions
  const searchSessions = useCallback(async (query: string) => {
    try {
      const results = await SessionService.searchSessions(query)
      return results
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to search sessions'
      setError(errorMessage)
      return []
    }
  }, [])

  // Filter and sort sessions
  const getFilteredSessions = useCallback((
    options: Partial<SessionListOptions> = {}
  ) => {
    const {
      sortBy = 'recent',
      groupBy = 'none',
      filters = {} as SessionFilters
    } = options

    let filtered = [...sessions]

    // Apply search filter
    if (filters.searchQuery) {
      const query = filters.searchQuery.toLowerCase()
      filtered = filtered.filter(session =>
        session.title.toLowerCase().includes(query) ||
        session.lastMessage.toLowerCase().includes(query) ||
        session.tags?.some(tag => tag.toLowerCase().includes(query))
      )
    }

    // Apply agent filter
    if (filters.agent) {
      filtered = filtered.filter(session => session.agent === filters.agent)
    }

    // Apply model filter
    if (filters.model) {
      filtered = filtered.filter(session => session.model === filters.model)
    }

    // Apply tags filter
    if (filters.tags && filters.tags.length > 0) {
      filtered = filtered.filter(session =>
        filters.tags!.every(tag => session.tags?.includes(tag))
      )
    }

    // Apply archived filter (show only active by default)
    if (!filters.dateRange) {
      filtered = filtered.filter(session => !session.isArchived)
    }

    // Sort sessions
    filtered.sort((a, b) => {
      // Pinned sessions always come first
      if (a.isPinned !== b.isPinned) {
        return a.isPinned ? -1 : 1
      }

      switch (sortBy) {
        case 'recent':
        case 'updated':
          return new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime()
        case 'name':
          return a.title.localeCompare(b.title)
        case 'messageCount':
          return b.messageCount - a.messageCount
        default:
          return 0
      }
    })

    return filtered
  }, [sessions])

  // Group sessions
  const getGroupedSessions = useCallback((
    filteredSessions: Session[],
    groupBy: string
  ) => {
    if (groupBy === 'none') {
      return { '': filteredSessions }
    }

    const groups: Record<string, Session[]> = {}

    filteredSessions.forEach(session => {
      let key = ''

      switch (groupBy) {
        case 'date':
          key = getGroupByDate(session.updatedAt)
          break
        case 'agent':
          key = session.agent
          break
        case 'model':
          key = session.model || 'Unknown'
          break
        default:
          key = ''
      }

      if (!groups[key]) {
        groups[key] = []
      }
      groups[key].push(session)
    })

    return groups
  }, [])

  // Load sessions on mount
  useEffect(() => {
    refreshSessions()
  }, [refreshSessions])

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      mountedRef.current = false
    }
  }, [])

  return {
    sessions,
    loading,
    error,
    refreshSessions,
    createSession,
    updateSession,
    deleteSession,
    togglePin,
    toggleArchive,
    addMessage,
    searchSessions,
    getFilteredSessions,
    getGroupedSessions,
  }
}

/**
 * Helper function to get group key by date
 */
function getGroupByDate(dateString: string): string {
  const date = new Date(dateString)
  const now = new Date()
  const diffDays = Math.floor((now.getTime() - date.getTime()) / (1000 * 60 * 60 * 24))

  if (diffDays === 0) return '今天'
  if (diffDays === 1) return '昨天'
  if (diffDays < 7) return '本周'
  if (diffDays < 30) return '本月'
  return '更早'
}
