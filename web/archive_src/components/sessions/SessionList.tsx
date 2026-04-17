import { useState, useMemo } from 'react'
import { cn } from '@/lib/utils'
import { SessionCard } from './SessionCard'
import type { Session, SessionSort } from '@/types/session'

interface SessionListProps {
  sessions: Session[]
  filterAgent?: string | null
  searchQuery?: string
  sortBy?: SessionSort
  selectedSessionId?: string | null
  onSessionClick: (sessionId: string) => void
  onSessionPin?: (sessionId: string) => void
  onSessionDelete?: (sessionId: string) => void
  onSessionMore?: (sessionId: string) => void
  className?: string
}

export function SessionList({
  sessions,
  filterAgent = null,
  searchQuery = '',
  sortBy = 'recent',
  selectedSessionId = null,
  onSessionClick,
  onSessionPin,
  onSessionDelete,
  onSessionMore,
  className
}: SessionListProps) {
  const [showPinnedOnly, setShowPinnedOnly] = useState(false)

  // Filter and sort sessions
  const filteredSessions = useMemo(() => {
    let filtered = sessions.filter(session => {
      // Filter by pinned status
      if (showPinnedOnly && !session.isPinned) return false

      // Filter by agent
      if (filterAgent && session.agent !== filterAgent) return false

      // Filter by search query
      if (searchQuery) {
        const query = searchQuery.toLowerCase()
        const titleMatch = session.title.toLowerCase().includes(query)
        const messageMatch = session.lastMessage.toLowerCase().includes(query)
        const tagMatch = session.tags?.some(tag => tag.toLowerCase().includes(query))
        
        if (!titleMatch && !messageMatch && !tagMatch) return false
      }

      return true
    })

    // Sort sessions
    return filtered.sort((a, b) => {
      // Pinned sessions always come first
      if (a.isPinned && !b.isPinned) return -1
      if (!a.isPinned && b.isPinned) return 1

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
  }, [sessions, filterAgent, searchQuery, sortBy, showPinnedOnly])

  // Group sessions by date
  const groupedSessions = useMemo(() => {
    const groups: Record<string, Session[]> = {}

    filteredSessions.forEach(session => {
      const date = new Date(session.updatedAt)
      const now = new Date()
      const diffTime = now.getTime() - date.getTime()
      const diffDays = Math.floor(diffTime / (1000 * 60 * 60 * 24))

      let groupKey = '更早'
      
      if (diffDays === 0) {
        groupKey = '今天'
      } else if (diffDays === 1) {
        groupKey = '昨天'
      } else if (diffDays < 7) {
        groupKey = '本周'
      } else if (diffDays < 30) {
        groupKey = '本月'
      }

      if (!groups[groupKey]) {
        groups[groupKey] = []
      }
      groups[groupKey].push(session)
    })

    return groups
  }, [filteredSessions])

  const hasPinnedSessions = sessions.some(s => s.isPinned)

  return (
    <div data-testid="session-list" className={cn('session-list', className)}>
      {/* Toolbar */}
      <div className="session-list__toolbar">
        <div className="session-list__search">
          <input
            type="text"
            placeholder="搜索会话..."
            value={searchQuery}
            readOnly
            className="session-list__search-input"
          />
        </div>

        {hasPinnedSessions && (
          <button
            className={cn(
              'session-list__filter-btn',
              showPinnedOnly && 'session-list__filter-btn--active'
            )}
            onClick={() => setShowPinnedOnly(!showPinnedOnly)}
            type="button"
          >
            {showPinnedOnly ? '🔒 仅显示固定' : '📌 显示全部'}
          </button>
        )}
      </div>

      {/* Session Groups */}
      {Object.entries(groupedSessions).map(([groupName, groupSessions]) => (
        <div key={groupName} className="session-list__group">
          <h3 className="session-list__group-title">
            {groupName} ({groupSessions.length})
          </h3>
          
          <div className="session-list__items">
            {groupSessions.map(session => (
              <SessionCard
                key={session.id}
                session={session}
                isSelected={selectedSessionId === session.id}
                onClick={() => onSessionClick(session.id)}
                onPin={onSessionPin}
                onDelete={onSessionDelete}
                onMore={onSessionMore}
              />
            ))}
          </div>
        </div>
      ))}

      {/* Empty State */}
      {filteredSessions.length === 0 && (
        <div className="session-list__empty">
          <p className="session-list__empty-message">
            {searchQuery ? '没有找到匹配的会话' : '暂无对话'}
          </p>
        </div>
      )}
    </div>
  )
}
