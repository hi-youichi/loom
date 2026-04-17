import type {
  AgentListResponse,
  AgentListSource,
  AgentSummary,
} from '@graphweave/protocol'
import { getConnection } from '@graphweave/ws-client'

export async function listAgents(options?: {
  sourceFilter?: AgentListSource
  workingFolder?: string
  sessionId?: string
}): Promise<AgentSummary[]> {
  const payload: Record<string, unknown> = { type: 'agent_list' }
  if (options?.sourceFilter) payload.source_filter = options.sourceFilter
  if (options?.workingFolder) payload.working_folder = options.workingFolder
  if (options?.sessionId) payload.thread_id = options.sessionId

  const resp = await getConnection().request(payload) as AgentListResponse
  return resp.agents
}
