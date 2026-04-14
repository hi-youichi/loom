import { useCallback, useMemo, useRef, useState, useEffect } from 'react'

import { ToolBlockAdapter } from '../adapters/ToolBlockAdapter'
import { ToolStreamAggregator } from '../adapters/ToolStreamAggregator'
import { sendMessage as sendChatMessage } from '../services/chat'
import { getUserMessages } from '../services/userMessages'
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
      return typeof event.agent === 'string' ? `${event.type} · ${event.agent}` : event.type
  }
}

export function useChat(options?: {
  sessionId?: string
  workspaceId?: string
  agentId?: string
  model?: string
}) {
  const sessionId = options?.sessionId
  const workspaceId = options?.workspaceId
  const agentId = options?.agentId || 'react'
  const model = options?.model

  const [messages, setMessages] = useState<UIMessageItemProps[]>([])
  const [isStreaming, setIsStreaming] = useState(false)
  const [connectionStatus, setConnectionStatus] = useState<WebSocketStatus>('connected')
  const [error, setError] = useState<string | null>(null)
  const [thinkingLines, setThinkingLines] = useState<string[]>([])
  const activeAssistantMessageIdRef = useRef<string | null>(null)
  const toolAggregatorRef = useRef(new ToolStreamAggregator())

  useEffect(() => {
    setMessages([])
    setThinkingLines([])
    toolAggregatorRef.current.reset()
    activeAssistantMessageIdRef.current = null
  }, [sessionId])

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
          sessionId,
          workspaceId,
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
      } finally {
        setIsStreaming(false)
      }
    },
    [isStreaming, sessionId, workspaceId, agentId, model, handleTextChunk, handleEvent, updateAssistantMessage],
  )

  const loadHistory = useCallback(async (targetSessionId?: string) => {
    const id = targetSessionId || sessionId
    if (!id) return

    try {
      const history = await getUserMessages(id)
      const uiMessages: UIMessageItemProps[] = []

      for (const msg of history) {
        uiMessages.push({
          id: crypto.randomUUID(),
          sender: msg.role === 'user' ? 'user' : 'assistant',
          timestamp: new Date().toISOString(),
          content: [
            {
              type: 'text' as const,
              text: msg.content,
              format: 'plain' as const,
            },
          ],
        })
      }

      setMessages(uiMessages)
    } catch {
      // silently fail - history loading is best-effort
    }
  }, [sessionId])

  return useMemo(
    () => ({
      messages,
      isStreaming,
      thinkingLines,
      connectionStatus,
      error,
      sendMessage,
      loadHistory,
    }),
    [connectionStatus, error, isStreaming, messages, sendMessage, thinkingLines, loadHistory],
  )
}
