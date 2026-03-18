import { useCallback, useMemo, useRef, useState } from 'react'

import { ToolBlockAdapter } from '../adapters/ToolBlockAdapter'
import { ToolStreamAggregator } from '../adapters/ToolStreamAggregator'
import { sendMessage as sendChatMessage } from '../services/chat'
import type {
  LoomStreamEvent,
  WebSocketStatus,
} from '../types/protocol/loom'
import { isToolEvent } from '../types/protocol/loom'
import type { UIMessageItemProps, UIToolContent } from '../types/ui/message'
import { useThread } from './useThread'

function createTextContent(text: string) {
  return {
    type: 'text' as const,
    text,
    format: 'plain' as const,
  }
}

function createUserMessage(text: string): UIMessageItemProps {
  return {
    id: crypto.randomUUID(),
    sender: 'user',
    timestamp: new Date().toISOString(),
    content: [createTextContent(text)],
  }
}

function createAssistantMessage(): UIMessageItemProps {
  return {
    id: crypto.randomUUID(),
    sender: 'assistant',
    timestamp: new Date().toISOString(),
    content: [],
  }
}

function upsertToolContent(
  content: UIMessageItemProps['content'],
  nextTool: UIToolContent,
): UIMessageItemProps['content'] {
  const existingIndex = content.findIndex((block) => block.type === 'tool' && block.id === nextTool.id)

  if (existingIndex === -1) {
    return [...content, nextTool]
  }

  return content.map((block, index) => (index === existingIndex ? nextTool : block))
}

function formatThinkLine(event: LoomStreamEvent): string | null {
  switch (event.type) {
    case 'run_start':
      return `run_start${typeof event.agent === 'string' ? ` · ${event.agent}` : ''}`
    case 'node_enter':
    case 'node_exit':
    case 'usage':
    case 'values':
    case 'updates':
    case 'checkpoint':
    case 'message_chunk':
    case 'thought_chunk':
    case 'tool_call_chunk':
    case 'tool_call':
    case 'tool_start':
    case 'tool_output':
    case 'tool_end':
      return null
    default:
      return typeof event.type === 'string' ? event.type : null
  }
}

export function useChat() {
  const { threadId } = useThread()
  const [messages, setMessages] = useState<UIMessageItemProps[]>([])
  const [isStreaming, setIsStreaming] = useState(false)
  const [connectionStatus, setConnectionStatus] = useState<WebSocketStatus>('connected')
  const [error, setError] = useState<string | null>(null)
  const [thinkingLines, setThinkingLines] = useState<string[]>([])
  const activeAssistantMessageIdRef = useRef<string | null>(null)
  const toolAggregatorRef = useRef(new ToolStreamAggregator())

  const updateAssistantMessage = useCallback(
    (updater: (message: UIMessageItemProps) => UIMessageItemProps) => {
      const messageId = activeAssistantMessageIdRef.current
      if (!messageId) {
        return
      }

      setMessages((current) =>
        current.map((message) => (message.id === messageId ? updater(message) : message)),
      )
    },
    [],
  )

  const handleTextChunk = useCallback(
    (chunk: string) => {
      updateAssistantMessage((message) => {
        const textIndex = message.content.findIndex((block) => block.type === 'text')

        if (textIndex === -1) {
          return {
            ...message,
            content: [createTextContent(chunk), ...message.content],
          }
        }

        return {
          ...message,
          content: message.content.map((block, index) =>
            index === textIndex && block.type === 'text'
              ? { ...block, text: block.text + chunk }
              : block,
          ),
        }
      })
    },
    [updateAssistantMessage],
  )

  const handleToolEvent = useCallback(
    (event: LoomStreamEvent) => {
      if (!isToolEvent(event)) {
        return
      }

      const toolState = toolAggregatorRef.current.apply(event)
      const toolContent = ToolBlockAdapter.toUI(toolState)

      updateAssistantMessage((message) => ({
        ...message,
        content: upsertToolContent(message.content, toolContent),
      }))
    },
    [updateAssistantMessage],
  )

  const handleEvent = useCallback(
    (event: LoomStreamEvent) => {
      handleToolEvent(event)

      if (event.type === 'thought_chunk' && event.content) {
        setThinkingLines((current) =>
          (current.join('\n') + event.content).split('\n'),
        )
        return
      }

      const thinkLine = formatThinkLine(event)
      if (thinkLine) {
        setThinkingLines((current) => [...current, thinkLine])
      }
    },
    [handleToolEvent],
  )

  const sendMessage = useCallback(
    async (text: string) => {
      const userMessage = createUserMessage(text)
      const assistantMessage = createAssistantMessage()

      activeAssistantMessageIdRef.current = assistantMessage.id
      toolAggregatorRef.current.reset()
      setThinkingLines([])
      setMessages((current) => [...current, userMessage, assistantMessage])
      setIsStreaming(true)
      setConnectionStatus('connecting')
      setError(null)

      try {
        const reply = await sendChatMessage(text, {
          threadId,
          onChunk: handleTextChunk,
          onEvent: handleEvent,
        })

        updateAssistantMessage((message) => {
          const hasTextBlock = message.content.some((block) => block.type === 'text')
          if (hasTextBlock || !reply.content) {
            return message
          }

          return {
            ...message,
            content: [createTextContent(reply.content), ...message.content],
          }
        })

        setConnectionStatus('connected')
      } catch (caughtError) {
        const nextError =
          caughtError instanceof Error
            ? caughtError.message
            : 'Request failed. Check whether `loom serve` is running.'

        setError(nextError)
        setConnectionStatus('error')
        throw caughtError
      } finally {
        setIsStreaming(false)
        activeAssistantMessageIdRef.current = null
      }
    },
    [handleEvent, handleTextChunk, threadId, updateAssistantMessage],
  )

  return useMemo(
    () => ({
      messages,
      isStreaming,
      thinkingLines,
      connectionStatus,
      error,
      sendMessage,
    }),
    [connectionStatus, error, isStreaming, messages, sendMessage, thinkingLines],
  )
}
