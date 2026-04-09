import { memo, useMemo } from 'react'
import type { AgentInfo, AgentStatus } from '@/types/agent'
import { AgentCard } from './AgentCard'

const STATUS_PRIORITY: Record<AgentStatus, number> = {
  running: 0,
  error: 1,
  idle: 2,
}

interface AgentGridProps {
  agents: AgentInfo[]
  selectedAgent: string | null
  onSelectAgent: (name: string | null) => void
}

export const AgentGrid = memo(function AgentGrid({
  agents,
  selectedAgent,
  onSelectAgent,
}: AgentGridProps) {
  const sorted = useMemo(
    () =>
      [...agents].sort((a, b) => {
        const pa = STATUS_PRIORITY[a.status]
        const pb = STATUS_PRIORITY[b.status]
        if (pa !== pb) return pa - pb
        return (b.lastRunAt ?? '').localeCompare(a.lastRunAt ?? '')
      }),
    [agents],
  )

  if (sorted.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-20 text-muted-foreground">
        <div className="size-12 rounded-full bg-muted flex items-center justify-center mb-3">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 8V4H8" /><rect width="16" height="12" x="4" y="8" rx="2" /><path d="M2 14h2" /><path d="M20 14h2" /><path d="M15 13v2" /><path d="M9 13v2" />
          </svg>
        </div>
        <p className="text-sm font-medium">暂无 Agent 活动</p>
        <p className="text-xs mt-1 text-muted-foreground/70">
          发送消息后，Agent 会自动出现在这里
        </p>
      </div>
    )
  }

  return (
    <div className="grid gap-3 grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
      {sorted.map((agent, i) => (
        <div
          key={agent.name}
          style={{ animationDelay: `${i * 40}ms` }}
          className="min-w-0 h-full animate-[fadeSlideUp_0.3s_ease-out_both]"
        >
          <AgentCard
            agent={agent}
            selected={selectedAgent === agent.name}
            onSelect={onSelectAgent}
          />
        </div>
      ))}
    </div>
  )
})
