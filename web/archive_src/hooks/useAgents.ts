import { useState, useEffect, useCallback, useRef } from 'react'
import type { AgentSource } from '../types/agent'
import type { AgentListSource, AgentSummary } from '../types/protocol/loom'
import type { AgentInfo } from '../types/agent'
import { listAgents } from '../services/agent'

const SOURCE_MAP: Record<string, AgentSource> = {
  builtin: 'builtin',
  project: 'project',
  user: 'user',
}

function toAgentInfo(summary: AgentSummary): AgentInfo {
  const source: AgentSource = SOURCE_MAP[summary.source] ?? 'builtin'
  return {
    name: summary.name,
    status: 'idle',
    callCount: 0,
    lastRunAt: null,
    lastError: null,
    profile: {
      name: summary.name,
      description: summary.description ?? `${summary.name} agent`,
      tools: [],
      mcpServers: [],
      source,
    },
  }
}

export function useAgents(options?: {
  sourceFilter?: AgentListSource
  workingFolder?: string
  sessionId?: string
  autoRefresh?: boolean
  refreshInterval?: number
}) {
  const [agents, setAgents] = useState<AgentInfo[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const mountedRef = useRef(true)
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const fetchAgents = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const summaries = await listAgents({
        sourceFilter: options?.sourceFilter,
        workingFolder: options?.workingFolder,
        sessionId: options?.sessionId,
      })
      if (mountedRef.current) {
        setAgents(summaries.map(toAgentInfo))
      }
    } catch (e: unknown) {
      if (mountedRef.current) {
        setError(e instanceof Error ? e.message : 'Failed to load agents')
      }
    } finally {
      if (mountedRef.current) {
        setLoading(false)
      }
    }
  }, [options?.sourceFilter, options?.workingFolder, options?.sessionId])

  useEffect(() => {
    fetchAgents()
  }, [fetchAgents])

  useEffect(() => {
    if (options?.autoRefresh && options.refreshInterval && options.refreshInterval > 0) {
      intervalRef.current = setInterval(fetchAgents, options.refreshInterval)
      return () => {
        if (intervalRef.current) clearInterval(intervalRef.current)
      }
    }
  }, [options?.autoRefresh, options?.refreshInterval, fetchAgents])

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
      if (intervalRef.current) clearInterval(intervalRef.current)
    }
  }, [])

  return { agents, loading, error, refetch: fetchAgents }
}