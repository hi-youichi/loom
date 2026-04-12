import { memo } from 'react'
import { cn } from '@/lib/utils'
import type { Session } from '@/types/session'
import { formatRelativeTime } from '@/utils/format'

interface SessionCardProps {
  session: Session
  isSelected?: boolean
  onClick: () => void
  onPin?: (sessionId: string) => void
  onDelete?: (sessionId: string) => void
  onMore?: (sessionId: string) => void
  className?: string
}

export const SessionCard = memo(function SessionCard({
  session,
  isSelected = false,
  onClick,
  onPin,
  onDelete,
  onMore,
  className
}: SessionCardProps) {
  const handlePin = (e: React.MouseEvent) => {
    e.stopPropagation()
    onPin?.(session.id)
  }

  const handleDelete = (e: React.MouseEvent) => {
    e.stopPropagation()
    onDelete?.(session.id)
  }

  const handleMore = (e: React.MouseEvent) => {
    e.stopPropagation()
    onMore?.(session.id)
  }

  return (
    <div
      className={cn(
        'session-card',
        isSelected && 'session-card--selected',
        session.isPinned && 'session-card--pinned',
        className
      )}
      onClick={onClick}
      role="button"
      tabIndex={0}
      aria-label={`会话: ${session.title}`}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault()
          onClick()
        }
      }}
    >
      <div className="session-card__header">
        <div className="session-card__title-row">
          {session.isPinned && (
            <span className="session-card__pin-icon" aria-label="已固定">📌</span>
          )}
          <h3 className="session-card__title">{session.title}</h3>
        </div>
        
        <div className="session-card__actions">
          {onPin && (
            <button
              className="session-card__action-btn"
              onClick={handlePin}
              aria-label={session.isPinned ? "取消固定" : "固定会话"}
              type="button"
            >
              {session.isPinned ? '🔓' : '📌'}
            </button>
          )}
          {onMore && (
            <button
              className="session-card__action-btn"
              onClick={handleMore}
              aria-label="更多选项"
              type="button"
            >
              ⋮
            </button>
          )}
        </div>
      </div>

      <p className="session-card__message">
        {session.lastMessage}
      </p>

      <div className="session-card__meta">
        <span className="session-card__badge" title={`Agent: ${session.agent}`}>
          🏷️ {session.agent}
        </span>
        <span className="session-card__badge" title={`Model: ${session.model}`}>
          🤖 {session.model}
        </span>
        <span className="session-card__badge" title={`${session.messageCount} 条消息`}>
          💬 {session.messageCount}
        </span>
        <span className="session-card__time" title={`更新于 ${new Date(session.updatedAt).toLocaleString()}`}>
          {formatRelativeTime(new Date(session.updatedAt))}
        </span>
      </div>

      {session.tags && session.tags.length > 0 && (
        <div className="session-card__tags">
          {session.tags.map((tag, index) => (
            <span key={index} className="session-card__tag">
              #{tag}
            </span>
          ))}
        </div>
      )}
    </div>
  )
})
