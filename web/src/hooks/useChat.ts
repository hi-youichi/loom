import { useCallback, useMemo, useRef, useState, useEffect } from 'react'

import { ToolBlockAdapter } from '../adapters/ToolBlockAdapter'
import { ToolStreamAggregator } from '../adapters/ToolStreamAggregator'
import { sendMessage as sendChatMessage } from '../services/chat'
import type {
  LoomStreamEvent,
  LoomToolEvent,
  WebSocketStatus,
} from '../types/protocol/loom'
import { isToolEvent } from '../types/protocol/loom'
import type { UIMessageItemProps, UIToolContent } from '../types/ui/message'

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

export function useChat(options?: {
  threadId?: string
  agentId?: string
  model?: string
}) {
  const threadId = options?.threadId
  const agentId = options?.agentId || 'react'
  const model = options?.model

  const [messages, setMessages] = useState<UIMessageItemProps[]>([])
  const [isStreaming, setIsStreaming] = useState(false)
  const [connectionStatus, setConnectionStatus] = useState<WebSocketStatus>('connected')
  const [error, setError] = useState<string | null>(null)
  const [thinkingLines, setThinkingLines] = useState<string[]>([])
  const activeAssistantMessageIdRef = useRef<string | null>(null)
  const toolAggregatorRef = useRef(new ToolStreamAggregator())

  // 当 threadId 变化时，清空消息
  useEffect(() => {
    setMessages([])
    setThinkingLines([])
    toolAggregatorRef.current.reset()
    activeAssistantMessageIdRef.current = null
  }, [threadId])

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
      updateAssistantMessage((msg) => ({
        ...msg,
        content: [
          ...msg.content.filter((block) => block.type !== 'text'),
          {
            type: 'text' as const,
            text: (msg.content.find((b) => b.type === 'text')?.type === 'text'
              ? (msg.content.find((b) => b.type === 'text') as { type: 'text'; text: string }).text
              : '') + chunk,
            format: 'plain' as const,
          },
        ],
      }))
    },
    [updateAssistantMessage],
  )

  const handleEvent = useCallback(
    (event: LoomStreamEvent) => {
      const thinkLine = formatThinkLine(event)
      if (thinkLine) {
        setThinkingLines((prev) => [...prev, thinkLine])
        return
      }

        if (isToolEvent(event)) {
          const nextTool = toolAggregatorRef.current.apply(event as LoomToolEvent)
          if (nextTool) {
            updateAssistantMessage((msg) => ({
              ...msg,
              content: upsertToolContent(msg.content, ToolBlockAdapter.toUI(nextTool)),
            }))
          }
        }
    },
    [updateAssistantMessage],
  )

  const sendMessage = useCallback(
    async (text: string) => {
      if (isStreaming) {
        return
      }

      const userMessage = createUserMessage(text)
      setMessages((prev) => [...prev, userMessage])

      const assistantMessage = createAssistantMessage()
      activeAssistantMessageIdRef.current = assistantMessage.id
      setMessages((prev) => [...prev, assistantMessage])

      setIsStreaming(true)
      setError(null)
      setConnectionStatus('connected')
      toolAggregatorRef.current.reset()

      try {
        const reply = await sendChatMessage(text, {
          threadId,
          agent: agentId,
          model,
          onChunk: handleTextChunk,
          onEvent: handleEvent,
        })
        if (reply.content) {
          updateAssistantMessage((msg) => ({
            ...msg,
            content: [
              ...msg.content.filter((block) => block.type !== 'text'),
              {
                type: 'text' as const,
                text: reply.content,
                format: 'plain' as const,
              },
            ],
          }))
        }
      } catch (caughtError) {
        let nextError = 'Request failed. Check whether `loom serve` is running.'
        if (caughtError instanceof Error) {
          nextError = caughtError.message
        }
        setError(nextError)
        setConnectionStatus('error')
        throw caughtError
      } finally {
        setIsStreaming(false)
        activeAssistantMessageIdRef.current = null
      }
    },
    [isStreaming, threadId, agentId, model, handleTextChunk, handleEvent],
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
