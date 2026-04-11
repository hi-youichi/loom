import { useState, useMemo } from 'react'
import { cn } from '@/lib/utils'
import { AgentGrid } from './AgentGrid'
import { ActivityFeed } from './ActivityFeed'
import { TabNavigator, type TabState, type TabConfig } from '../tabs/TabNavigator'
import { TabContent, TabPane } from '../tabs/TabContent'
import { SessionList } from '../sessions/SessionList'
import type { AgentInfo, ActivityEvent } from '@/types/agent'
import type { Session } from '@/types/session'

interface DashboardViewProps {
  agents: AgentInfo[]
  activity: ActivityEvent[]
  activeCount: number
  totalCalls: number
  sessions?: Session[]  // New optional prop
}

export function DashboardView({ 
  agents, 
  activity, 
  activeCount, 
  totalCalls,
  sessions = []
}: DashboardViewProps) {
  const [selectedAgent, setSelectedAgent] = useState<string | null>(null)
  
  // Tab state with persistence
  const [activeTab, setActiveTab] = useState<TabState>(() => {
    return (localStorage.getItem('dashboard-last-tab') as TabState) || 'sessions'
  })

  const handleSelectAgent = (name: string | null) => {
    if (name == null) {
      setSelectedAgent(null)
    } else {
      setSelectedAgent((prev) => (prev === name ? null : name))
    }
  }

  const handleTabChange = (tab: TabState) => {
    setActiveTab(tab)
    localStorage.setItem('dashboard-last-tab', tab)
  }

  const handleSessionClick = (sessionId: string) => {
    // Navigate to session
    console.log('Navigate to session:', sessionId)
    // TODO: Implement navigation logic
  }

  const handleSessionPin = (sessionId: string) => {
    console.log('Pin session:', sessionId)
    // TODO: Implement pin logic
  }

  const handleSessionDelete = (sessionId: string) => {
    if (confirm('确定要删除这个会话吗？')) {
      console.log('Delete session:', sessionId)
      // TODO: Implement delete logic
    }
  }

  const handleSessionMore = (sessionId: string) => {
    console.log('Show more options for session:', sessionId)
    // TODO: Implement context menu
  }

  // Tab configurations
  const tabs: TabConfig[] = useMemo(() => [
    {
      id: 'sessions',
      label: '最近会话',
      icon: '💬',
      badge: sessions.length
    },
    {
      id: 'activity',
      label: '最近活动',
      icon: '📊',
      badge: activity.length
    }
  ], [sessions.length, activity.length])

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
                管理 AI Agent 会话与活动
              </p>
            </div>
            <div className="flex items-center gap-3">
              <StatChip label="活跃" value={activeCount} accent={activeCount > 0} />
              <StatChip label="Agents" value={agents.length} accent={false} />
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
              <span className="text-muted-foreground ml-0.5">×</span>
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
          {/* Tab Navigation */}
          <div className="shrink-0 pt-4">
            <TabNavigator
              tabs={tabs}
              activeTab={activeTab}
              onTabChange={handleTabChange}
              variant="underline"
              size="md"
            />
          </div>

          {/* Tab Content */}
          <TabContent activeTab={activeTab} animation="fade" className="flex-1 min-h-0 overflow-hidden">
            <TabPane tabId="sessions" className="h-full overflow-y-auto">
              <SessionList
                sessions={sessions}
                filterAgent={selectedAgent}
                selectedSessionId={null}
                onSessionClick={handleSessionClick}
                onSessionPin={handleSessionPin}
                onSessionDelete={handleSessionDelete}
                onSessionMore={handleSessionMore}
                className="py-4"
              />
            </TabPane>
            
            <TabPane tabId="activity" className="h-full overflow-y-auto">
              <div className="shrink-0 py-3 flex items-center justify-between">
                <h2 className="text-xs font-semibold text-muted-foreground uppercase tracking-widest">
                  活动记录
                </h2>
                {selectedAgent && (
                  <span className="text-[0.65rem] text-muted-foreground">
                    筛选: {selectedAgent}
                  </span>
                )}
              </div>
              <ActivityFeed events={activity} filterAgent={selectedAgent} />
            </TabPane>
          </TabContent>
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
