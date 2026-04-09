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

type SendMessageOptions = {
  threadId?: string
  workspaceId?: string
  agent?: string          // ← 新增：执行模式，默认 'react'
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
    // agent can be either a builtin type (react/dup/tot/got) or custom profile name (dev/assistant/ask)
    agent: agentValue,
    thread_id: options.threadId,
    working_folder: workingFolder,
    verbose: false,
  }
  if (options.workspaceId) payload.workspace_id = options.workspaceId

  return getConnection().request(payload, onMessage).then(
    () => ({ content: streamedReply }),
  )
}
