import { useCallback, useMemo, useState } from 'react'

import type {
  LoomStreamEvent,
  LoomRunStartEvent,
  LoomMessageChunkEvent,
  LoomThoughtChunkEvent,
  LoomToolCallEvent,
  LoomToolStartEvent,
  LoomToolOutputEvent,
  LoomToolEndEvent,
} from '../types/protocol/loom'
import type { AgentInfo, AgentStatus, ActivityEvent } from '../types/agent'

const MAX_ACTIVITY_EVENTS = 200

function getEventSummary(event: LoomStreamEvent): string | null {
  switch (event.type) {
    case 'run_start': {
      const e = event as LoomRunStartEvent
      return e.message ?? null
    }
    case 'message_chunk': {
      const e = event as LoomMessageChunkEvent
      return e.content.length > 60 ? e.content.slice(0, 60) + '...' : e.content
    }
    case 'thought_chunk': {
      const e = event as LoomThoughtChunkEvent
      return e.content.length > 60 ? e.content.slice(0, 60) + '...' : e.content
    }
    case 'tool_call': {
      const e = event as LoomToolCallEvent
      return e.name
    }
    case 'tool_start': {
      const e = event as LoomToolStartEvent
      return e.name ?? null
    }
    case 'tool_output': {
      const e = event as LoomToolOutputEvent
      return e.content.length > 80 ? e.content.slice(0, 80) + '...' : e.content
    }
    case 'tool_end': {
      const e = event as LoomToolEndEvent
      const name = e.name ?? 'tool'
      return e.is_error ? `${name} failed` : `${name} done`
    }
    default:
      return null
  }
}

function isEventError(event: LoomStreamEvent): boolean {
  return event.type === 'tool_end' && event.is_error === true
}

export function useAgents() {
  const [agents, setAgents] = useState<Map<string, AgentInfo>>(new Map())
  const [agentOrder, setAgentOrder] = useState<string[]>([])
  const [activity, setActivity] = useState<ActivityEvent[]>([])

  const applyEvent = useCallback((event: LoomStreamEvent) => {
    const agentName =
      typeof (event as Record<string, unknown>).agent === 'string'
        ? (event as Record<string, unknown>).agent as string
        : null

    const now = new Date().toISOString()

    if (agentName) {
      setAgents((prev) => {
        const next = new Map(prev)
        const existing = next.get(agentName)

        if (event.type === 'run_start') {
          if (existing) {
            next.set(agentName, {
              ...existing,
              status: 'running' as AgentStatus,
              callCount: existing.callCount + 1,
              lastRunAt: now,
            })
          } else {
            next.set(agentName, {
              name: agentName,
              status: 'running' as AgentStatus,
              callCount: 1,
              lastRunAt: now,
              lastError: null,
            })
          }
        } else if (event.type === 'tool_end' && event.is_error && existing) {
          next.set(agentName, {
            ...existing,
            status: 'error' as AgentStatus,
            lastError: getEventSummary(event),
          })
        }

        return next
      })

      if (event.type === 'run_start') {
        setAgentOrder((prev) =>
          prev.includes(agentName) ? prev : [...prev, agentName],
        )
      }
    }

    if (agentName) {
      const actEvent: ActivityEvent = {
        id: crypto.randomUUID(),
        timestamp: now,
        agent: agentName,
        type: event.type,
        summary: getEventSummary(event),
        isError: isEventError(event),
      }
      setActivity((prev) => [actEvent, ...prev].slice(0, MAX_ACTIVITY_EVENTS))
    }
  }, [])

  const reset = useCallback(() => {
    setAgents(new Map())
    setActivity([])
    setAgentOrder([])
  }, [])

  const agentList = useMemo(() => {
    return agentOrder.map((name) => agents.get(name)).filter((a): a is AgentInfo => a != null)
  }, [agents, agentOrder])

  const activeCount = useMemo(
    () => agentList.filter((a) => a.status === 'running').length,
    [agentList],
  )

  const totalCalls = useMemo(
    () => agentList.reduce((sum, a) => sum + a.callCount, 0),
    [agentList],
  )

  return useMemo(
    () => ({
      agents: agentList,
      activity,
      activeCount,
      totalCalls,
      applyEvent,
      reset,
    }),
    [activity, agentList, activeCount, totalCalls, applyEvent, reset],
  )
}
