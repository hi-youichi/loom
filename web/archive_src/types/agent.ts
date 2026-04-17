export type AgentStatus = 'running' | 'idle' | 'error'

export type AgentSource = 'builtin' | 'project' | 'user'

export interface AgentProfile {
  name: string
  description: string | null
  graphPattern?: string
  tools: string[]
  mcpServers: string[]
  source: AgentSource
}

export interface AgentInfo {
  name: string
  status: AgentStatus
  callCount: number
  lastRunAt: string | null
  lastError: string | null
  profile?: AgentProfile
}

export type ActivityEvent = {
  id: string
  timestamp: string
  agent: string
  type: string
  summary: string | null
  isError: boolean
}
