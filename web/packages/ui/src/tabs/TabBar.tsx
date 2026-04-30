import { memo } from 'react'
import { X } from 'lucide-react'
import { cn } from '../lib/utils'

export interface TabBarTab {
  id: string
  title: string
  closable?: boolean
}

export interface TabBarProps {
  tabs: TabBarTab[]
  activeId: string
  onSelect: (id: string) => void
  onClose: (id: string) => void
  className?: string
}

export const TabBar = memo(function TabBar({
  tabs,
  activeId,
  onSelect,
  onClose,
  className,
}: TabBarProps) {
  return (
    <div
      className={cn(
        'flex items-center border-b border-border bg-background',
        className,
      )}
    >
      {tabs.map((tab) => (
        <div
          key={tab.id}
          className={cn(
            'group flex items-center gap-1.5 px-3 py-1.5 text-xs border-r border-border cursor-pointer transition-colors',
            activeId === tab.id
              ? 'bg-accent/60 font-medium'
              : 'hover:bg-accent/20 text-muted-foreground',
          )}
          onClick={() => onSelect(tab.id)}
        >
          <span className="truncate max-w-[120px]">{tab.title}</span>
          {tab.closable && (
            <button
              type="button"
              className="opacity-0 group-hover:opacity-100 p-0.5 rounded hover:bg-muted transition-all"
              onClick={(e) => {
                e.stopPropagation()
                onClose(tab.id)
              }}
            >
              <X className="size-3" />
            </button>
          )}
        </div>
      ))}
    </div>
  )
})
