import { useState, useEffect, useCallback, useRef } from 'react'
import { SessionService } from '../service'
import type { Session, SessionFilter, SessionListOptions } from '@loom/types'

export function useSessions() {
  const [sessions, setSessions] = useState<Session[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const mountedRef = useRef(true)

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

  const deleteSession = useCallback(async (id: string) => {
    try {
      await SessionService.deleteSession(id)
      if (mountedRef.current) {
        setSessions(prev => prev.filter(session => session.id !== id))
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to delete session'
      setError(errorMessage)
      throw err
    }
  }, [])

  const togglePin = useCallback(async (id: string) => {
    try {
      const updated = await SessionService.togglePin(id)
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

  const toggleArchive = useCallback(async (id: string) => {
    try {
      const updated = await SessionService.toggleArchive(id)
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

  const addMessage = useCallback(async (sessionId: string, message: string) => {
    try {
      const updated = await SessionService.addMessage(sessionId, message)
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

  const getFilteredSessions = useCallback((
    options: Partial<SessionListOptions> = {}
  ) => {
    const {
      sortBy = 'recent',
      filters = {} as SessionFilter,
    } = options

    let filtered = [...sessions]

    if (filters.searchQuery) {
      const query = filters.searchQuery.toLowerCase()
      filtered = filtered.filter(session =>
        session.title.toLowerCase().includes(query) ||
        session.lastMessage.toLowerCase().includes(query) ||
        session.tags?.some((tag: string) => tag.toLowerCase().includes(query))
      )
    }

    if (filters.agent) {
      filtered = filtered.filter(session => session.agent === filters.agent)
    }

    if (filters.model) {
      filtered = filtered.filter(session => session.model === filters.model)
    }

    if (filters.tags && filters.tags.length > 0) {
      filtered = filtered.filter(session =>
        filters.tags!.every((tag: string) => session.tags?.includes(tag))
      )
    }

    if (!filters.dateRange) {
      filtered = filtered.filter(session => !session.isArchived)
    }

    filtered.sort((a, b) => {
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

  const getGroupedSessions = useCallback((
    filteredSessions: Session[],
    groupBy: string
  ) => {
    if (groupBy === 'none') {
      return { '': filteredSessions }
    }

    const groups: Record<string, Session[]> = {}

    for (const session of filteredSessions) {
      let groupKey: string

      switch (groupBy) {
        case 'date':
          groupKey = getGroupByDate(session.updatedAt)
          break
        case 'agent':
          groupKey = session.agent
          break
        case 'model':
          groupKey = session.model
          break
        default:
          groupKey = '其他'
      }

      if (!groups[groupKey]) {
        groups[groupKey] = []
      }
      groups[groupKey].push(session)
    }

    return groups
  }, [])

  useEffect(() => {
    mountedRef.current = true
    refreshSessions()
    return () => {
      mountedRef.current = false
    }
  }, [refreshSessions])

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
