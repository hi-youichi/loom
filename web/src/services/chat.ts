import type {
  ChatReply,
  LoomServerMessage,
  LoomStreamEvent,
} from '../types/protocol/loom'
import {
  isMessageChunkEvent,
  isRunEnd,
  isRunStreamEvent,
} from '../types/protocol/loom'
import { getConnection } from './connection'
import { setSessionModel } from './model'

type SendMessageOptions = {
  sessionId?: string
  workspaceId?: string
  agent?: string
  model?: string
  onChunk?: (chunk: string) => void
  onEvent?: (event: LoomStreamEvent) => void
}

function getEnvValue(name: string) {
  return (import.meta.env as Record<string, string | undefined>)[name]?.trim()
}

function getWorkingFolder() {
  const value = getEnvValue('VITE_LOOM_WORKING_FOLDER')
  return value && value.length > 0 ? value : undefined
}

export function sendMessage(
  content: string,
  options: SendMessageOptions = {},
): Promise<ChatReply> {
  let streamedReply = ''
  let runId: string | null = null

  const onMessage = (msg: LoomServerMessage): boolean => {
    if (isRunStreamEvent(msg)) {
      runId ??= msg.id
      if (msg.id !== runId) return false

      options.onEvent?.(msg.event)

      if (isMessageChunkEvent(msg.event) && msg.event.content) {
        streamedReply += msg.event.content
        options.onChunk?.(msg.event.content)
      }
      return false
    }

    if (isRunEnd(msg)) {
      runId ??= msg.id
      if (msg.id !== runId) return false
      streamedReply = msg.reply || streamedReply
      return true
    }

    return false
  }

  const workingFolder = getWorkingFolder()
  const agentValue = options.agent || 'dev'

  const payload: Record<string, unknown> = {
    type: 'run',
    message: content,
    agent: agentValue,
    thread_id: options.sessionId,
    working_folder: workingFolder,
    verbose: false,
    model: options.model,
  }
  if (options.workspaceId) payload.workspace_id = options.workspaceId

  return getConnection().request(payload, onMessage).then(
    () => ({ content: streamedReply }),
  )
}

export async function sendMessageWithModel(
  content: string,
  modelId: string,
  options: SendMessageOptions = {}
): Promise<ChatReply> {
  const reply = await sendMessage(content, { ...options, model: modelId })

  if (options.sessionId && modelId) {
    try {
      await setSessionModel(modelId, options.sessionId)
    } catch (error) {
      console.warn('Failed to set session model:', error)
    }
  }

  return reply
}
