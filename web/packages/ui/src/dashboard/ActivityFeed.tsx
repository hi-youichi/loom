import { memo, useEffect, useRef } from 'react'
import { cn } from '../lib/utils'
import { formatRelativeTime } from '@graphweave/utils'
import type { ActivityEvent } from '@graphweave/types'

interface ActivityFeedProps {
  events: ActivityEvent[]
  filterAgent: string | null
}

const EVENT_CONFIG: Record<string, { icon: string; style: string; label: string }> = {
  run_start: {
    icon: '▶',
    style: 'text-emerald-600 bg-emerald-500/8',
    label: 'start',
  },
  run_end: {
    icon: '■',
    style: 'text-zinc-500 bg-zinc-500/8',
    label: 'end',
  },
  tool_start: {
    icon: '⚡',
    style: 'text-sky-600 bg-sky-500/8',
    label: 'tool',
  },
  tool_end: {
    icon: '✓',
    style: 'text-sky-600 bg-sky-500/8',
    label: 'tool',
  },
  tool_call: {
    icon: '⚡',
    style: 'text-sky-600 bg-sky-500/8',
    label: 'tool',
  },
  tool_output: {
    icon: '↩',
    style: 'text-zinc-500 bg-zinc-500/8',
    label: 'output',
  },
  message_chunk: {
    icon: '💬',
    style: 'text-foreground bg-muted',
    label: 'msg',
  },
  thought_chunk: {
    icon: '💭',
    style: 'text-violet-600 bg-violet-500/8',
    label: 'think',
  },
}

const DEFAULT_EVENT_CONFIG = { icon: '•', style: 'text-muted-foreground bg-muted', label: 'event' }

export const ActivityFeed = memo(function ActivityFeed({ events, filterAgent }: ActivityFeedProps) {
  const scrollRef = useRef<HTMLDivElement>(null)

  const filtered = filterAgent ? events.filter((e) => e.agent === filterAgent) : events

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = 0
    }
  }, [filtered.length])

  if (events.length === 0) {
    return (
      <div className="flex-1 min-h-0 flex items-center justify-center py-16 text-muted-foreground">
        <div className="text-center">
          <p className="text-sm font-medium">暂无活动记录</p>
          <p className="text-xs mt-1 text-muted-foreground/70">Agent 开始运行后会显示事件流</p>
        </div>
      </div>
    )
  }

  if (filtered.length === 0) {
    return (
      <div className="flex-1 min-h-0 flex items-center justify-center py-16 text-muted-foreground">
        <p className="text-sm">该 Agent 暂无活动记录</p>
      </div>
    )
  }

  return (
    <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto">
      <div className="flex flex-col">
        {filtered.map((event) => {
          const cfg = EVENT_CONFIG[event.type] ?? DEFAULT_EVENT_CONFIG
          const isError = event.isError

          return (
            <div
              key={event.id}
              className={cn(
                'group flex items-start gap-3 px-8 py-2.5 text-xs transition-colors',
                'border-b border-border/30 hover:bg-accent/30',
                isError && 'bg-red-50/60 hover:bg-red-50/80',
              )}
            >
              <span className="shrink-0 text-muted-foreground/60 tabular-nums w-14 pt-px">
                {formatRelativeTime(event.timestamp)}
              </span>
              <span className="shrink-0 font-medium text-foreground/80 w-24 truncate pt-px">
                {event.agent}
              </span>
              <span
                className={cn(
                  'shrink-0 px-1.5 py-0.5 rounded-md font-mono text-[0.6rem] font-medium leading-none',
                  isError ? 'text-red-600 bg-red-500/10' : cfg.style,
                )}
              >
                <span className="mr-1">{isError ? '✕' : cfg.icon}</span>
                {isError ? 'error' : cfg.label}
              </span>
              {event.summary && (
                <span
                  className={cn(
                    'min-w-0 truncate pt-px',
                    isError ? 'text-red-600 font-medium' : 'text-muted-foreground',
                  )}
                >
                  {event.summary}
                </span>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
})
