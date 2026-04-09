import type {
  WorkspaceListResponse,
  WorkspaceCreateResponse,
  WorkspaceThreadListResponse,
  WorkspaceThreadAddResponse,
  WorkspaceThreadRemoveResponse,
  WorkspaceMeta,
  ThreadInWorkspace,
  LoomServerMessage,
} from '../types/protocol/loom'
import { isWorkspaceResponse, isError } from '../types/protocol/loom'

function getEnvValue(name: string) {
  return (import.meta.env as Record<string, string | undefined>)[name]?.trim()
}

function getLoomWsUrl() {
  return getEnvValue('VITE_LOOM_WS_URL') || 'ws://127.0.0.1:8080'
}

function sendWorkspaceRequest<T>(request: object): Promise<T> {
  return new Promise((resolve: (value: T) => void, reject: (reason: Error) => void) => {
    const socket = new WebSocket(getLoomWsUrl())
    let settled = false

    const cleanup = () => {
      socket.removeEventListener('open', handleOpen)
      socket.removeEventListener('message', handleMessage)
      socket.removeEventListener('error', handleError)
      socket.removeEventListener('close', handleClose)
    }

    const fail = (error: Error) => {
      if (settled) return
      settled = true
      cleanup()
      if (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING) {
        socket.close()
      }
      reject(error)
    }

    const finish = (result: T) => {
      if (settled) return
      settled = true
      cleanup()
      if (socket.readyState === WebSocket.OPEN) {
        socket.close()
      }
      resolve(result)
    }

    const handleOpen = () => {
      socket.send(JSON.stringify(request))
    }

    const handleMessage = (event: MessageEvent<string>) => {
      if (typeof event.data !== 'string') return

      let msg: LoomServerMessage
      try {
        msg = JSON.parse(event.data) as LoomServerMessage
      } catch {
        fail(new Error('Failed to parse workspace response.'))
        return
      }

      if (isError(msg)) {
        fail(new Error(msg.error || 'Unknown workspace error.'))
        return
      }

      if (isWorkspaceResponse(msg)) {
        finish(msg as T)
        return
      }
    }

    const handleError = () => {
      fail(new Error(`Unable to reach Loom WebSocket server at ${getLoomWsUrl()}.`))
    }

    const handleClose = () => {
      if (!settled) {
        fail(new Error('WebSocket connection closed before workspace response arrived.'))
      }
    }

    socket.addEventListener('open', handleOpen)
    socket.addEventListener('message', handleMessage)
    socket.addEventListener('error', handleError)
    socket.addEventListener('close', handleClose)
  })
}

export async function listWorkspaces(): Promise<WorkspaceMeta[]> {
  const resp = await sendWorkspaceRequest<WorkspaceListResponse>({
    type: 'workspace_list',
    id: crypto.randomUUID(),
  })
  return resp.workspaces
}

export async function createWorkspace(name?: string): Promise<string> {
  const resp = await sendWorkspaceRequest<WorkspaceCreateResponse>({
    type: 'workspace_create',
    id: crypto.randomUUID(),
    ...(name ? { name } : {}),
  })
  return resp.workspace_id
}

export async function listThreads(workspaceId: string): Promise<ThreadInWorkspace[]> {
  const resp = await sendWorkspaceRequest<WorkspaceThreadListResponse>({
    type: 'workspace_thread_list',
    id: crypto.randomUUID(),
    workspace_id: workspaceId,
  })
  return resp.threads
}

export async function addThread(workspaceId: string, threadId: string): Promise<void> {
  await sendWorkspaceRequest<WorkspaceThreadAddResponse>({
    type: 'workspace_thread_add',
    id: crypto.randomUUID(),
    workspace_id: workspaceId,
    thread_id: threadId,
  })
}

export async function removeThread(workspaceId: string, threadId: string): Promise<void> {
  await sendWorkspaceRequest<WorkspaceThreadRemoveResponse>({
    type: 'workspace_thread_remove',
    id: crypto.randomUUID(),
    workspace_id: workspaceId,
    thread_id: threadId,
  })
}