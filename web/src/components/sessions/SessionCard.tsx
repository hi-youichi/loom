/**
 * SessionCard component - displays a single session in a card format
 */

import { useState } from 'react'
import { Pin, Trash2, MoreVertical, Archive, RotateCcw } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { Session } from '@/types/session'
import {
  formatRelativeTime,
  getSessionDisplayName,
  getAgentDisplayName,
  getModelDisplayName,
  formatPreviewText,
} from '@/utils/session'

interface SessionCardProps {
  session: Session
  isSelected: boolean
  onClick: () => void
  onPin: (id: string) => void
  onDelete: (id: string) => void
  onArchive?: (id: string) => void
  onDuplicate?: (id: string) => void
  className?: string
}

export function SessionCard({
  session,
  isSelected,
  onClick,
  onPin,
  onDelete,
  onArchive,
  onDuplicate,
  className,
}: SessionCardProps) {
  const [showMenu, setShowMenu] = useState(false)

  const handleMenuClick = (e: React.MouseEvent) => {
    e.stopPropagation()
    setShowMenu(!showMenu)
  }

  const handleActionClick = (
    e: React.MouseEvent,
    action: () => void
  ) => {
    e.stopPropagation()
    action()
    setShowMenu(false)
  }

  return (
    <div
      className={cn(
        'group relative rounded-lg border transition-all duration-200',
        'hover:shadow-md',
        isSelected
          ? 'border-primary bg-primary/5 shadow-sm'
          : 'border-border bg-card hover:border-primary/50',
        'cursor-pointer',
        className
      )}
      onClick={onClick}
    >
      {/* Card Header */}
      <div className="flex items-start gap-3 p-4">
        {/* Left side: Title and preview */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <h3
              className={cn(
                'font-medium truncate',
                isSelected ? 'text-foreground' : 'text-foreground'
              )}
            >
              {getSessionDisplayName(session)}
            </h3>
            {session.isPinned && (
              <Pin className="h-3.5 w-3.5 text-primary shrink-0" />
            )}
          </div>
          
          <p className="text-sm text-muted-foreground line-clamp-2 mb-2">
            {formatPreviewText(session.lastMessage, 80)}
          </p>
          
          {/* Metadata */}
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <span className="px-2 py-0.5 rounded-full bg-secondary/50">
              {getAgentDisplayName(session.agent)}
            </span>
            <span>•</span>
            <span>{getModelDisplayName(session.model)}</span>
            <span>•</span>
            <span>{formatRelativeTime(session.updatedAt)}</span>
          </div>
        </div>

        {/* Right side: Action button */}
        <div className="relative">
          <button
            onClick={handleMenuClick}
            className={cn(
              'p-1.5 rounded-md transition-colors',
              'hover:bg-accent hover:text-accent-foreground',
              'opacity-0 group-hover:opacity-100',
              showMenu && 'opacity-100'
            )}
            aria-label="更多选项"
          >
            <MoreVertical className="h-4 w-4" />
          </button>

          {/* Dropdown menu */}
          {showMenu && (
            <div
              className={cn(
                'absolute right-0 top-full mt-1 w-40',
                'bg-popover border border-border rounded-md shadow-lg',
                'z-50 py-1'
              )}
              onClick={(e) => e.stopPropagation()}
            >
              <button
                onClick={(e) => handleActionClick(e, () => onPin(session.id))}
                className="w-full px-3 py-2 text-left text-sm hover:bg-accent flex items-center gap-2"
              >
                <Pin className="h-3.5 w-3.5" />
                {session.isPinned ? '取消固定' : '固定'}
              </button>

              {onArchive && (
                <button
                  onClick={(e) => handleActionClick(e, () => onArchive(session.id))}
                  className="w-full px-3 py-2 text-left text-sm hover:bg-accent flex items-center gap-2"
                >
                  <Archive className="h-3.5 w-3.5" />
                  {session.isArchived ? '取消归档' : '归档'}
                </button>
              )}

              {onDuplicate && (
                <button
                  onClick={(e) => handleActionClick(e, () => onDuplicate(session.id))}
                  className="w-full px-3 py-2 text-left text-sm hover:bg-accent flex items-center gap-2"
                >
                  <RotateCcw className="h-3.5 w-3.5" />
                  复制
                </button>
              )}

              <div className="border-t border-border my-1" />

              <button
                onClick={(e) => handleActionClick(e, () => onDelete(session.id))}
                className="w-full px-3 py-2 text-left text-sm hover:bg-destructive/10 hover:text-destructive flex items-center gap-2"
              >
                <Trash2 className="h-3.5 w-3.5" />
                删除
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Tags */}
      {session.tags && session.tags.length > 0 && (
        <div className="px-4 pb-3 pt-0">
          <div className="flex flex-wrap gap-1.5">
            {session.tags.slice(0, 3).map((tag) => (
              <span
                key={tag}
                className="px-2 py-0.5 text-xs rounded-full bg-primary/10 text-primary"
              >
                {tag}
              </span>
            ))}
            {session.tags.length > 3 && (
              <span className="px-2 py-0.5 text-xs rounded-full bg-muted text-muted-foreground">
                +{session.tags.length - 3}
              </span>
            )}
          </div>
        </div>
      )}

      {/* Message count indicator */}
      <div className="absolute bottom-2 right-2 px-2 py-0.5 rounded-full bg-secondary/50 text-xs text-muted-foreground">
        {session.messageCount} 条消息
      </div>
    </div>
  )
}
