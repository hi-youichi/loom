import type {
  ChatReply,
  LoomServerMessage,
  LoomStreamEvent,
} from '../types/protocol/loom'
import {
  isError,
  isMessageChunkEvent,
  isRunEnd,
  isRunStreamEvent,
} from '../types/protocol/loom'

type SendMessageOptions = {
  threadId?: string
  onChunk?: (chunk: string) => void
  onEvent?: (event: LoomStreamEvent) => void
}

function getEnvValue(name: string) {
  return (import.meta.env as Record<string, string | undefined>)[name]?.trim()
}

function getLoomWsUrl() {
  return getEnvValue('VITE_LOOM_WS_URL') || 'ws://127.0.0.1:8080'
}

function getWorkingFolder() {
  const value = getEnvValue('VITE_LOOM_WORKING_FOLDER')
  return value && value.length > 0 ? value : undefined
}

function parseServerMessage(data: string): LoomServerMessage {
  return JSON.parse(data) as LoomServerMessage
}

export function sendMessage(
  content: string,
  options: SendMessageOptions = {},
): Promise<ChatReply> {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(getLoomWsUrl())
    const workingFolder = getWorkingFolder()
    const request = {
      type: 'run',
      message: content,
      agent: 'react',
      thread_id: options.threadId,
      working_folder: workingFolder,
      verbose: false,
    }

    let settled = false
    let runId: string | null = null
    let streamedReply = ''

    const cleanup = () => {
      socket.removeEventListener('open', handleOpen)
      socket.removeEventListener('message', handleMessage)
      socket.removeEventListener('error', handleError)
      socket.removeEventListener('close', handleClose)
    }

    const fail = (error: Error) => {
      if (settled) {
        return
      }

      settled = true
      cleanup()

      if (
        socket.readyState === WebSocket.OPEN ||
        socket.readyState === WebSocket.CONNECTING
      ) {
        socket.close()
      }

      reject(error)
    }

    const finish = (reply: string) => {
      if (settled) {
        return
      }

      settled = true
      cleanup()

      if (socket.readyState === WebSocket.OPEN) {
        socket.close()
      }

      resolve({ content: reply })
    }

    const handleOpen = () => {
      socket.send(JSON.stringify(request))
    }

    const handleMessage = (event: MessageEvent<string>) => {
      if (typeof event.data !== 'string') {
        fail(new Error('Received a non-text WebSocket frame from Loom server.'))
        return
      }

      let message: LoomServerMessage

      try {
        message = parseServerMessage(event.data)
      } catch {
        fail(new Error('Failed to parse Loom server response.'))
        return
      }

      if (isRunStreamEvent(message)) {
        runId ??= message.id
        if (message.id !== runId) {
          return
        }

        options.onEvent?.(message.event)

        if (isMessageChunkEvent(message.event) && message.event.content) {
          streamedReply += message.event.content
          options.onChunk?.(message.event.content)
        }
        return
      }

      if (isRunEnd(message)) {
        runId ??= message.id
        if (message.id !== runId) {
          return
        }

        finish(message.reply || streamedReply)
        return
      }

      if (isError(message)) {
        if (message.id && runId && message.id !== runId) {
          return
        }

        fail(new Error(message.error))
      }
    }

    const handleError = () => {
      fail(
        new Error(
          `Unable to reach Loom WebSocket server at ${getLoomWsUrl()}. Start it with \`loom serve\`.`,
        ),
      )
    }

    const handleClose = () => {
      if (settled) {
        return
      }

      if (streamedReply) {
        finish(streamedReply)
        return
      }

      fail(new Error('Loom WebSocket connection closed before run_end arrived.'))
    }

    socket.addEventListener('open', handleOpen)
    socket.addEventListener('message', handleMessage)
    socket.addEventListener('error', handleError)
    socket.addEventListener('close', handleClose)
  })
}
