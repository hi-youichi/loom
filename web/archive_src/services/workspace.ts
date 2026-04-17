import type {
  WorkspaceListResponse,
  WorkspaceCreateResponse,
  WorkspaceSessionListResponse,
  WorkspaceSessionAddResponse,
  WorkspaceSessionRemoveResponse,
  WorkspaceMeta,
  SessionInWorkspace,
} from '../types/protocol/loom'
import { getConnection } from './connection'

function request<T>(payload: object): Promise<T> {
  return getConnection().request({
    ...payload,
    id: crypto.randomUUID(),
  }).then(msg => msg as T)
}

export async function listWorkspaces(): Promise<WorkspaceMeta[]> {
  const resp = await request<WorkspaceListResponse>({
    type: 'workspace_list',
  })
  return resp.workspaces
}

export async function createWorkspace(name?: string): Promise<string> {
  const resp = await request<WorkspaceCreateResponse>({
    type: 'workspace_create',
    ...(name ? { name } : {}),
  })
  return resp.workspace_id
}

export async function listSessions(workspaceId: string): Promise<SessionInWorkspace[]> {
  const resp = await request<WorkspaceSessionListResponse>({
    type: 'workspace_thread_list',
    workspace_id: workspaceId,
  })
  return resp.threads
}

export async function addSession(workspaceId: string, sessionId: string): Promise<void> {
  await request<WorkspaceSessionAddResponse>({
    type: 'workspace_thread_add',
    workspace_id: workspaceId,
    thread_id: sessionId,
  })
}

export async function removeSession(workspaceId: string, sessionId: string): Promise<void> {
  await request<WorkspaceSessionRemoveResponse>({
    type: 'workspace_thread_remove',
    workspace_id: workspaceId,
    thread_id: sessionId,
  })
}

export async function listThreads(workspaceId: string): Promise<SessionInWorkspace[]> {
  return listSessions(workspaceId)
}

export async function addThread(workspaceId: string, threadId: string): Promise<void> {
  return addSession(workspaceId, threadId)
}

export async function removeThread(workspaceId: string, threadId: string): Promise<void> {
  return removeSession(workspaceId, threadId)
}
