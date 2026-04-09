import type {
  WorkspaceListResponse,
  WorkspaceCreateResponse,
  WorkspaceThreadListResponse,
  WorkspaceThreadAddResponse,
  WorkspaceThreadRemoveResponse,
  WorkspaceMeta,
  ThreadInWorkspace,
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

export async function listThreads(workspaceId: string): Promise<ThreadInWorkspace[]> {
  const resp = await request<WorkspaceThreadListResponse>({
    type: 'workspace_thread_list',
    workspace_id: workspaceId,
  })
  return resp.threads
}

export async function addThread(workspaceId: string, threadId: string): Promise<void> {
  await request<WorkspaceThreadAddResponse>({
    type: 'workspace_thread_add',
    workspace_id: workspaceId,
    thread_id: threadId,
  })
}

export async function removeThread(workspaceId: string, threadId: string): Promise<void> {
  await request<WorkspaceThreadRemoveResponse>({
    type: 'workspace_thread_remove',
    workspace_id: workspaceId,
    thread_id: threadId,
  })
}
