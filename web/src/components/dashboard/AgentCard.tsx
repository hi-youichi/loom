import { memo } from 'react'
import { cn } from '@/lib/utils'
import type { AgentInfo, AgentSource, AgentStatus } from '@/types/agent'

const STATUS_CONFIG: Record<AgentStatus, { dot: string; label: string; ring: string; cardBorder: string }> = {
  running: {
    dot: 'bg-emerald-400',
    label: '运行中',
    ring: 'ring-1 ring-emerald-400/30',
    cardBorder: 'border-emerald-500/20',
  },
  idle: {
    dot: 'bg-zinc-400',
    label: '空闲',
    ring: '',
    cardBorder: '',
  },
  error: {
    dot: 'bg-red-500',
    label: '错误',
    ring: 'ring-1 ring-red-500/25',
    cardBorder: 'border-red-500/20',
  },
}


const SOURCE_CONFIG: Record<AgentSource, { style: string; label: string }> = {
  builtin: {
    style: 'bg-zinc-100 text-zinc-500',
    label: 'builtin',
  },
  project: {
    style: 'bg-sky-50 text-sky-600',
    label: 'project',
  },
  user: {
    style: 'bg-violet-50 text-violet-600',
    label: 'user',
  },
}

function formatRelativeTime(iso: string | null): string {
  if (!iso) return '-'
  const diff = Date.now() - new Date(iso).getTime()
  if (diff < 1000) return '刚刚'
  if (diff < 60_000) return `${Math.floor(diff / 1000)}s前`
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m前`
  return `${Math.floor(diff / 3_600_000)}h前`
}

const MAX_VISIBLE_TOOLS = 3

interface AgentCardProps {
  agent: AgentInfo
  selected: boolean
  onSelect: (name: string) => void
}

export const AgentCard = memo(function AgentCard({ agent, selected, onSelect }: AgentCardProps) {
  const statusConfig = STATUS_CONFIG[agent.status]
  const profile = agent.profile
  const allTools = profile?.tools ?? []
  const source = profile?.source ?? 'builtin'
  const hasError = agent.status === 'error' && agent.lastError
  const sourceConfig = SOURCE_CONFIG[source]

  const visibleTools = allTools.slice(0, MAX_VISIBLE_TOOLS)
  const overflowCount = allTools.length - MAX_VISIBLE_TOOLS

  return (
    <button
      type="button"
      onClick={() => onSelect(agent.name)}
      className={cn(
        'group relative flex flex-col min-w-0 w-full rounded-xl border bg-card p-4 text-left transition-all duration-200',
        'hover:shadow-sm hover:-translate-y-px',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
        statusConfig.cardBorder,
        selected
          ? 'border-foreground/20 bg-accent/80 shadow-sm'
          : 'border-border/80',
      )}
    >
      <div className="flex items-start justify-between gap-2 mb-3">
        <div className="flex items-center gap-2 min-w-0">
          <span
            className={cn(
              'size-2.5 rounded-full shrink-0 transition-colors',
              statusConfig.dot,
              agent.status === 'running' && 'animate-pulse',
            )}
          />
          <span className="text-sm font-semibold leading-tight truncate">
            {agent.name}
          </span>
        </div>

      </div>

      <div className="flex items-center justify-between text-xs text-muted-foreground mb-3">
        <span>
          <span className="font-medium text-foreground tabular-nums">{agent.callCount}</span>
          <span className="ml-1">次调用</span>
        </span>
        <span className="tabular-nums">{formatRelativeTime(agent.lastRunAt)}</span>
      </div>

      <div className="flex-1 min-h-[2.5rem] flex items-start">
        {hasError ? (
          <div className="rounded-md bg-red-50 border border-red-200 px-2.5 py-1.5 w-full min-w-0">
            <p className="text-[0.7rem] text-red-600 font-medium truncate">
              {agent.lastError}
            </p>
          </div>
        ) : visibleTools.length > 0 ? (
          <div className="flex flex-wrap gap-1">
            {visibleTools.map((tool) => (
              <span
                key={tool}
                className="text-[0.6rem] font-mono px-1.5 py-0.5 rounded-md bg-muted/60 text-muted-foreground"
              >
                {tool}
              </span>
            ))}
            {overflowCount > 0 && (
              <span className="text-[0.6rem] font-mono px-1.5 py-0.5 rounded-md bg-muted/60 text-muted-foreground/70">
                +{overflowCount}
              </span>
            )}
          </div>
        ) : null}
      </div>

      <div className="mt-auto pt-2 border-t border-border/40">
        <span
          className={cn(
            'text-[0.6rem] font-medium px-2 py-0.5 rounded-full',
            sourceConfig.style,
          )}
        >
          {sourceConfig.label}
        </span>
      </div>
    </button>
  )
})
