import { useState } from 'react'
import { DashboardView } from '../components/dashboard/DashboardView'
import { mockSessions } from '../data/mockSessions'
import type { AgentInfo, ActivityEvent } from '../types/agent'

export function DashboardDemo() {
  // Mock data
  const agents: AgentInfo[] = [
    { name: 'dev', status: 'idle', callCount: 42, lastRunAt: '2024-01-15T14:30:00Z', lastError: null },
    { name: 'ask', status: 'idle', callCount: 28, lastRunAt: '2024-01-14T16:00:00Z', lastError: null },
    { name: 'explore', status: 'idle', callCount: 15, lastRunAt: '2024-01-13T18:30:00Z', lastError: null }
  ]

  const activity: ActivityEvent[] = [
    {
      id: 'event-1',
      timestamp: new Date(Date.now() - 5 * 60 * 1000).toISOString(),
      agent: 'dev',
      type: 'tool_call',
      summary: 'read src/main.ts',
      isError: false
    },
    {
      id: 'event-2',
      timestamp: new Date(Date.now() - 10 * 60 * 1000).toISOString(),
      agent: 'dev',
      type: 'message_chunk',
      summary: '我建议使用 TypeScript 重构整个项目...',
      isError: false
    },
    {
      id: 'event-3',
      timestamp: new Date(Date.now() - 30 * 60 * 1000).toISOString(),
      agent: 'ask',
      type: 'tool_call',
      summary: 'search API authentication methods',
      isError: false
    }
  ]

  return (
    <div style={{ height: '100vh', display: 'flex', flexDirection: 'column' }}>
      <DashboardView
        agents={agents}
        activity={activity}
        activeCount={2}
        totalCalls={85}
        sessions={mockSessions}
      />
    </div>
  )
}
