/**
 * Loom 协议类型定义
 * 这些类型直接对应后端 WebSocket 流式协议。
 */

export type WebSocketStatus = 'connecting' | 'connected' | 'disconnected' | 'error'

type LoomEnvelope = {
  session_id?: string
  node_id?: string
  event_id?: number
}

export type LoomToolStatus = 'queued' | 'running' | 'done' | 'error'

export type LoomMessageChunkEvent = LoomEnvelope & {
  type: 'message_chunk'
  content: string
  id?: string
}

/** Chunk of reasoning/thinking (ACP agent_thought_chunk). Use for thinking UI only. */
export type LoomThoughtChunkEvent = LoomEnvelope & {
  type: 'thought_chunk'
  content: string
  id?: string
}

export type LoomToolCallChunkEvent = LoomEnvelope & {
  type: 'tool_call_chunk'
  call_id?: string
  name?: string
  arguments_delta: string
}

export type LoomToolCallEvent = LoomEnvelope & {
  type: 'tool_call'
  call_id?: string
  name: string
  arguments: unknown
}

export type LoomToolStartEvent = LoomEnvelope & {
  type: 'tool_start'
  call_id?: string
  name?: string
}

export type LoomToolOutputEvent = LoomEnvelope & {
  type: 'tool_output'
  call_id?: string
  name?: string
  content: string
}

export type LoomToolEndEvent = LoomEnvelope & {
  type: 'tool_end'
  call_id?: string
  name?: string
  result: unknown
  is_error: boolean
}

export type LoomUsageEvent = LoomEnvelope & {
  type: 'usage'
  prompt_tokens: number
  completion_tokens: number
  total_tokens: number
}

export type LoomRunStartEvent = LoomEnvelope & {
  type: 'run_start'
  run_id?: string
  message?: string
  agent?: string
}

export type LoomNodeEnterEvent = LoomEnvelope & {
  type: 'node_enter'
  id: string
}

export type LoomNodeExitEvent = LoomEnvelope & {
  type: 'node_exit'
  id: string
  result: unknown
}

export type LoomValuesEvent = LoomEnvelope & {
  type: 'values'
  state: unknown
}

export type LoomUpdatesEvent = LoomEnvelope & {
  type: 'updates'
  id?: string
  state: unknown
}

export type LoomCheckpointEvent = LoomEnvelope & {
  type: 'checkpoint'
  checkpoint_id?: string
  timestamp?: string
  step?: number
  state?: unknown
  thread_id?: string
  checkpoint_ns?: string
}

export type LoomUnknownEvent = LoomEnvelope & {
  type: string
  [key: string]: unknown
}

export type LoomToolEvent =
  | LoomToolCallChunkEvent
  | LoomToolCallEvent
  | LoomToolStartEvent
  | LoomToolOutputEvent
  | LoomToolEndEvent

export type LoomStreamEvent =
  | LoomRunStartEvent
  | LoomNodeEnterEvent
  | LoomNodeExitEvent
  | LoomMessageChunkEvent
  | LoomThoughtChunkEvent
  | LoomUsageEvent
  | LoomValuesEvent
  | LoomUpdatesEvent
  | LoomCheckpointEvent
  | LoomToolEvent
  | LoomUnknownEvent

export interface LoomRunStreamEventResponse {
  type: 'run_stream_event'
  id: string
  event: LoomStreamEvent
}

export interface LoomRunEndResponse {
  type: 'run_end'
  id: string
  reply: string
}

export interface LoomErrorResponse {
  type: 'error'
  id?: string
  error: string
}

export type WorkspaceListRequest = {
  type: 'workspace_list'
  id: string
}

export type WorkspaceCreateRequest = {
  type: 'workspace_create'
  id: string
  name?: string
}

export type WorkspaceThreadListRequest = {
  type: 'workspace_thread_list'
  id: string
  workspace_id: string
}

export type WorkspaceThreadAddRequest = {
  type: 'workspace_thread_add'
  id: string
  workspace_id: string
  thread_id: string
}

export type WorkspaceThreadRemoveRequest = {
  type: 'workspace_thread_remove'
  id: string
  workspace_id: string
  thread_id: string
}

export type WorkspaceRequest =
  | WorkspaceListRequest
  | WorkspaceCreateRequest
  | WorkspaceThreadListRequest
  | WorkspaceThreadAddRequest
  | WorkspaceThreadRemoveRequest

export type WorkspaceMeta = {
  id: string
  name?: string | null
  created_at_ms: number
}

export type ThreadInWorkspace = {
  thread_id: string
  created_at_ms: number
}

export type WorkspaceListResponse = {
  type: 'workspace_list'
  id: string
  workspaces: WorkspaceMeta[]
}

export type WorkspaceCreateResponse = {
  type: 'workspace_create'
  id: string
  workspace_id: string
}

export type WorkspaceThreadListResponse = {
  type: 'workspace_thread_list'
  id: string
  workspace_id: string
  threads: ThreadInWorkspace[]
}

export type WorkspaceThreadAddResponse = {
  type: 'workspace_thread_add'
  id: string
  workspace_id: string
  thread_id: string
}

export type WorkspaceThreadRemoveResponse = {
  type: 'workspace_thread_remove'
  id: string
  workspace_id: string
  thread_id: string
}

export type WorkspaceResponse =
  | WorkspaceListResponse
  | WorkspaceCreateResponse
  | WorkspaceThreadListResponse
  | WorkspaceThreadAddResponse
  | WorkspaceThreadRemoveResponse

export type AgentListSource = 'builtin' | 'project' | 'user'

export type AgentSummary = {
  name: string
  description: string | null
  source: AgentListSource
}

export type AgentListRequest = {
  type: 'agent_list'
  id: string
  source_filter?: AgentListSource
  working_folder?: string
  thread_id?: string
}

export type AgentListResponse = {
  type: 'agent_list'
  id: string
  agents: AgentSummary[]
}

export type LoomServerMessage =
  | LoomRunStreamEventResponse
  | LoomRunEndResponse
  | LoomErrorResponse
  | AgentListResponse
  | WorkspaceListResponse
  | WorkspaceCreateResponse
  | WorkspaceThreadListResponse
  | WorkspaceThreadAddResponse
  | WorkspaceThreadRemoveResponse
  | { type: string }

export function isRunStreamEvent(msg: LoomServerMessage): msg is LoomRunStreamEventResponse {
  return msg.type === 'run_stream_event'
}

export function isRunEnd(msg: LoomServerMessage): msg is LoomRunEndResponse {
  return msg.type === 'run_end'
}

export function isError(msg: LoomServerMessage): msg is LoomErrorResponse {
  return msg.type === 'error'
}

export function isMessageChunkEvent(event: LoomStreamEvent): event is LoomMessageChunkEvent {
  return event.type === 'message_chunk'
}

export function isThoughtChunkEvent(event: LoomStreamEvent): event is LoomThoughtChunkEvent {
  return event.type === 'thought_chunk'
}

export function isToolEvent(event: LoomStreamEvent): event is LoomToolEvent {
  return (
    event.type === 'tool_call_chunk' ||
    event.type === 'tool_call' ||
    event.type === 'tool_start' ||
    event.type === 'tool_output' ||
    event.type === 'tool_end'
  )
}

export interface ChatReply {
  content: string
}

export function isWorkspaceResponse(msg: LoomServerMessage): msg is WorkspaceResponse {
  return typeof msg.type === 'string' && msg.type.startsWith('workspace_')
}
