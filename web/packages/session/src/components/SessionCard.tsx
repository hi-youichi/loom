import { memo } from 'react'
import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'
import type { Session } from '@loom/types'
import { formatSessionRelativeTime } from '../utils'

function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

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

  const _handleDelete = (e: React.MouseEvent) => {
    e.stopPropagation()
    onDelete?.(session.id)
  }
  void _handleDelete

  const handleMore = (e: React.MouseEvent) => {
    e.stopPropagation()
    onMore?.(session.id)
  }

  return (
    <div
      data-testid={`session-card-${session.id}`}
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
          <h3 data-testid="session-card__title" className="session-card__title">{session.title}</h3>
        </div>

        <div className="session-card__actions">
          {onPin && (
            <button
              data-testid="pin-session-btn"
              className="session-card__action-btn"
              onClick={handlePin}
              aria-label={session.isPinned ? "取消固定" : "固定"}
              type="button"
            >
              {session.isPinned ? '🔒' : '📌'}
            </button>
          )}
          {onMore && (
            <button
              data-testid="more-session-btn"
              className="session-card__action-btn"
              onClick={handleMore}
              aria-label="更多选项"
              type="button"
            >
              •••
            </button>
          )}
        </div>
      </div>

      <p data-testid="session-card__last-message" className="session-card__message">
        {session.lastMessage}
      </p>

      <div className="session-card__meta">
        <span className="session-card__badge" title={`Agent: ${session.agent}`}>
          🤖 {session.agent}
        </span>
        <span className="session-card__badge" title={`Model: ${session.model}`}>
          🧠 {session.model}
        </span>
        <span data-testid="session-card__count" className="session-card__badge" title={`${session.messageCount} 条消息`}>
          💬 {session.messageCount}
        </span>
        <span className="session-card__time" title={`更新于 ${new Date(session.updatedAt).toLocaleString()}`}>
          {formatSessionRelativeTime(session.updatedAt)}
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
