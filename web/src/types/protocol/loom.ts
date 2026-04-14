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

export interface CancelRunRequest {
  type: 'cancel_run'
  id: string
  run_id: string
}

export interface CancelRunResponse {
  type: 'cancel_run_ack'
  id: string
  run_id: string
}

export interface ChatReply {
  content: string
}

// -----------------------------------------------------------------------------
// Workspace types
// -----------------------------------------------------------------------------

export type WorkspaceListRequest = {
  type: 'workspace_list'
  id: string
}

export type WorkspaceCreateRequest = {
  type: 'workspace_create'
  id: string
  name?: string
}

export type WorkspaceSessionListRequest = {
  type: 'workspace_thread_list'
  id: string
  workspace_id: string
}

export type WorkspaceSessionAddRequest = {
  type: 'workspace_thread_add'
  id: string
  workspace_id: string
  thread_id: string
}

export type WorkspaceSessionRemoveRequest = {
  type: 'workspace_thread_remove'
  id: string
  workspace_id: string
  thread_id: string
}

export type WorkspaceRequest =
  | WorkspaceListRequest
  | WorkspaceCreateRequest
  | WorkspaceSessionListRequest
  | WorkspaceSessionAddRequest
  | WorkspaceSessionRemoveRequest

export type WorkspaceMeta = {
  id: string
  name?: string | null
  created_at_ms: number
}

export type SessionInWorkspace = {
  thread_id: string
  created_at_ms: number
}

export type ThreadInWorkspace = SessionInWorkspace

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

export type WorkspaceSessionListResponse = {
  type: 'workspace_thread_list'
  id: string
  workspace_id: string
  threads: SessionInWorkspace[]
}

export type WorkspaceSessionAddResponse = {
  type: 'workspace_thread_add'
  id: string
  workspace_id: string
  thread_id: string
}

export type WorkspaceSessionRemoveResponse = {
  type: 'workspace_thread_remove'
  id: string
  workspace_id: string
  thread_id: string
}

export type WorkspaceResponse =
  | WorkspaceListResponse
  | WorkspaceCreateResponse
  | WorkspaceSessionListResponse
  | WorkspaceSessionAddResponse
  | WorkspaceSessionRemoveResponse

export type WorkspaceThreadListResponse = WorkspaceSessionListResponse
export type WorkspaceThreadAddResponse = WorkspaceSessionAddResponse
export type WorkspaceThreadRemoveResponse = WorkspaceSessionRemoveResponse

// -----------------------------------------------------------------------------
// Agent types
// -----------------------------------------------------------------------------

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

// -----------------------------------------------------------------------------
// Model types
// -----------------------------------------------------------------------------

export type ModelInfo = {
  id: string
  name: string
  provider: string
  family?: string
  capabilities?: string[]
}

export type ListModelsRequest = {
  type: 'list_models'
  id: string
}

export type ListModelsResponse = {
  type: 'models_list'
  id: string
  models: ModelInfo[]
}

export type SetModelRequest = {
  type: 'set_model'
  id: string
  model_id: string
  session_id?: string
}

export type SetModelResponse = {
  type: 'model_set'
  id: string
  success: boolean
  error?: string
}

// -----------------------------------------------------------------------------
// Server message types
// -----------------------------------------------------------------------------

export type LoomServerMessage =
  | LoomRunStreamEventResponse
  | LoomRunEndResponse
  | LoomErrorResponse
  | AgentListResponse
  | ListModelsResponse
  | SetModelResponse
  | WorkspaceListResponse
  | WorkspaceCreateResponse
  | WorkspaceSessionListResponse
  | WorkspaceSessionAddResponse
  | WorkspaceSessionRemoveResponse
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

export function isWorkspaceResponse(msg: LoomServerMessage): msg is WorkspaceResponse {
  return (
    msg.type === 'workspace_list' ||
    msg.type === 'workspace_create' ||
    msg.type === 'workspace_thread_list' ||
    msg.type === 'workspace_thread_add' ||
    msg.type === 'workspace_thread_remove'
  )
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