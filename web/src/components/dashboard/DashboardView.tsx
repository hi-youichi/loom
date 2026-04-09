import { useState } from 'react'
import { cn } from '@/lib/utils'
import { AgentGrid } from './AgentGrid'
import { ActivityFeed } from './ActivityFeed'
import type { AgentInfo, ActivityEvent } from '@/types/agent'

interface DashboardViewProps {
  agents: AgentInfo[]
  activity: ActivityEvent[]
  activeCount: number
  totalCalls: number
}

export function DashboardView({ agents, activity, activeCount, totalCalls }: DashboardViewProps) {
  const [selectedAgent, setSelectedAgent] = useState<string | null>(null)

  const handleSelectAgent = (name: string | null) => {
    if (name == null) {
      setSelectedAgent(null)
    } else {
      setSelectedAgent((prev) => (prev === name ? null : name))
    }
  }

  return (
    <div className="h-full flex flex-col overflow-hidden">
      <div className="max-w-6xl mx-auto w-full px-6 pb-4 flex-1 min-h-0 flex flex-col">
        <header className="shrink-0 z-10 pt-6 pb-4 border-b border-border/60 bg-gradient-to-b from-background via-background to-background/95">
          <div className="flex items-center justify-between mb-1">
            <div>
              <h1
                className="text-2xl font-bold tracking-tight"
                style={{ fontFamily: "'Fraunces', serif" }}
              >
                Agent Dashboard
              </h1>
              <p className="text-sm text-muted-foreground mt-1">
                监控和管理你的 AI Agent 运行状态
              </p>
            </div>
            <div className="flex items-center gap-3">
              <StatChip label="活跃" value={activeCount} accent={activeCount > 0} />
              <StatChip label="总计" value={agents.length} accent={false} />
              <StatChip label="调用" value={totalCalls} accent={false} />
            </div>
          </div>

          {selectedAgent && (
            <button
              type="button"
              className={cn(
                'mt-3 inline-flex items-center gap-1.5 text-xs font-medium px-2.5 py-1 rounded-full',
                'bg-accent text-accent-foreground border border-border',
                'hover:bg-accent/80 transition-colors',
              )}
              onClick={() => setSelectedAgent(null)}
            >
              <span className="size-1.5 rounded-full bg-foreground/40" />
              筛选: {selectedAgent}
              <span className="text-muted-foreground ml-0.5">✕</span>
            </button>
          )}
        </header>

        <section className="shrink-0 py-4 overflow-y-auto">
          <AgentGrid
            agents={agents}
            selectedAgent={selectedAgent}
            onSelectAgent={handleSelectAgent}
          />
        </section>

        <section className="flex-1 min-h-0 border-t border-border/60 flex flex-col">
          <div className="shrink-0 py-3 flex items-center justify-between">
            <h2 className="text-xs font-semibold text-muted-foreground uppercase tracking-widest">
              最近活动
            </h2>
            {selectedAgent && (
              <span className="text-[0.65rem] text-muted-foreground">
                仅显示 {selectedAgent}
              </span>
            )}
          </div>
          <ActivityFeed events={activity} filterAgent={selectedAgent} />
        </section>
      </div>
    </div>
  )
}

function StatChip({ label, value, accent }: { label: string; value: number; accent: boolean }) {
  return (
    <div className="flex flex-col items-center px-3.5 py-1.5 rounded-lg bg-muted/40 border border-border/40">
      <span
        className={cn(
          'text-base font-semibold tabular-nums leading-tight',
          accent ? 'text-emerald-600' : 'text-foreground',
        )}
      >
        {value}
      </span>
      <span className="text-[0.6rem] text-muted-foreground font-medium">{label}</span>
    </div>
  )
}
