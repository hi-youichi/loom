import { Fragment, useMemo, useState } from 'react'

import { MessageComposer } from '../components/MessageComposer'
import { ThinkIndicator } from '../components/ThinkIndicator'
import { sendMessage } from '../services/chat'
import type { Message, MessageBlock } from '../types/chat'

const THREAD_STORAGE_KEY = 'loom-web-thread-id'

function getOrCreateThreadId() {
  const existing = window.localStorage.getItem(THREAD_STORAGE_KEY)
  if (existing) {
    return existing
  }

  const nextThreadId = crypto.randomUUID()
  window.localStorage.setItem(THREAD_STORAGE_KEY, nextThreadId)
  return nextThreadId
}

function formatTime(timestamp: string) {
  return new Intl.DateTimeFormat('en', {
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(timestamp))
}

function createTextMessage(role: Message['role'], text: string, createdAt: string): Message {
  return {
    id: crypto.randomUUID(),
    role,
    createdAt,
    blocks: text
      ? [
          {
            id: crypto.randomUUID(),
            type: 'text',
            text,
          },
        ]
      : [],
  }
}

function isTextBlock(block: MessageBlock): block is Extract<MessageBlock, { type: 'text' }> {
  return block.type === 'text'
}

function getMessageText(message: Message) {
  return message.blocks
    .filter(isTextBlock)
    .map((block) => block.text)
    .join('')
}

function appendMessageChunk(message: Message, chunk: string): Message {
  const textBlock = message.blocks.find((block) => block.type === 'text')

  if (!textBlock) {
    return {
      ...message,
      blocks: [
        ...message.blocks,
        {
          id: crypto.randomUUID(),
          type: 'text',
          text: chunk,
        },
      ],
    }
  }

  return {
    ...message,
    blocks: message.blocks.map((block) =>
      block.id === textBlock.id && block.type === 'text'
        ? { ...block, text: block.text + chunk }
        : block,
    ),
  }
}

function replaceMessageText(message: Message, text: string): Message {
  const textBlock = message.blocks.find((block) => block.type === 'text')

  if (!textBlock) {
    return {
      ...message,
      blocks: [
        ...message.blocks,
        {
          id: crypto.randomUUID(),
          type: 'text',
          text,
        },
      ],
    }
  }

  return {
    ...message,
    blocks: message.blocks.map((block) =>
      block.id === textBlock.id && block.type === 'text' ? { ...block, text } : block,
    ),
  }
}

type ThinkingState = {
  lines: string[]
  active: boolean
}

type UserEvent = {
  type: 'user'
  id: string
  createdAt: string
  text: string
}

type AssistantTextEvent = {
  type: 'assistant_text'
  id: string
  createdAt: string
  text: string
}

type AssistantThinkingEvent = {
  type: 'assistant_thinking'
  id: string
  lines: string[]
  active: boolean
}

type StreamEvent = UserEvent | AssistantTextEvent | AssistantThinkingEvent

function formatThinkLine(event: Record<string, unknown>) {
  switch (event.type) {
    case 'run_start':
      return `run_start${typeof event.agent === 'string' ? ` · ${event.agent}` : ''}`
    case 'tool_call':
      return `tool_call${typeof event.name === 'string' ? ` · ${event.name}` : ''}`
    case 'tool_start':
      return `tool_start${typeof event.name === 'string' ? ` · ${event.name}` : ''}`
    case 'tool_output':
      if (typeof event.name === 'string' && typeof event.content === 'string') {
        return `tool_output · ${event.name} · ${event.content}`
      }
      return `tool_output${typeof event.name === 'string' ? ` · ${event.name}` : ''}`
    case 'tool_end': {
      const name = typeof event.name === 'string' ? ` · ${event.name}` : ''
      const result = typeof event.result === 'string' ? ` · ${event.result}` : ''
      return `tool_end${name}${result}`
    }
    case 'node_enter':
    case 'node_exit':
    case 'usage':
    case 'values':
    case 'updates':
    case 'checkpoint':
    case 'message_chunk':
      return null
    default:
      return typeof event.type === 'string' ? event.type : null
  }
}

export function ChatPage() {
  const threadId = useMemo(() => getOrCreateThreadId(), [])
  const [streamEvents, setStreamEvents] = useState<StreamEvent[]>([])
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleSend = async (text: string) => {
    const createdAt = new Date().toISOString()
    const userId = crypto.randomUUID()
    const thinkingId = crypto.randomUUID()
    const textId = crypto.randomUUID()

    // Add user message and assistant placeholder events
    setStreamEvents((current) => [
      ...current,
      { type: 'user', id: userId, createdAt, text },
      { type: 'assistant_thinking', id: thinkingId, lines: [], active: true },
      { type: 'assistant_text', id: textId, createdAt, text: '' },
    ])
    setSending(true)
    setError(null)

    try {
      const reply = await sendMessage(text, {
        threadId,
        onEvent: (event) => {
          const line = formatThinkLine(event)
          if (!line) {
            return
          }

          setStreamEvents((current) =>
            current.map((e) =>
              e.type === 'assistant_thinking' && e.id === thinkingId
                ? { ...e, lines: [...e.lines, line] }
                : e,
            ),
          )
        },
        onChunk: (chunk) => {
          setStreamEvents((current) =>
            current.map((e) =>
              e.type === 'assistant_text' && e.id === textId
                ? { ...e, text: e.text + chunk }
                : e,
            ),
          )
        },
      })

      // Mark thinking as inactive and finalize text
      setStreamEvents((current) =>
        current.map((e) => {
          if (e.type === 'assistant_thinking' && e.id === thinkingId) {
            return { ...e, active: false }
          }
          if (e.type === 'assistant_text' && e.id === textId) {
            return { ...e, text: reply.content || e.text }
          }
          return e
        }),
      )
    } catch (caughtError) {
      const nextError =
        caughtError instanceof Error
          ? caughtError.message
          : 'Request failed. Check whether `loom serve` is running.'

      // Remove assistant events on error
      setStreamEvents((current) =>
        current.filter((e) => !(e.type === 'assistant_thinking' && e.id === thinkingId) && !(e.type === 'assistant_text' && e.id === textId)),
      )
      setError(nextError)
      throw caughtError
    } finally {
      setSending(false)
    }
  }

  return (
    <main className="shell">
      <section className="chat-panel" aria-label="Chat demo">
        <div className="message-list" role="log" aria-live="polite">
          {streamEvents.map((event) => {
            if (event.type === 'user') {
              return (
                <article key={event.id} className="message message--user" aria-label="user message">
                  <div className="message__meta">
                    <time dateTime={event.createdAt}>{formatTime(event.createdAt)}</time>
                  </div>
                  <p className="message__content">{event.text}</p>
                </article>
              )
            }

            if (event.type === 'assistant_thinking') {
              return (
                <ThinkIndicator key={event.id} lines={event.lines} active={event.active} />
              )
            }

            if (event.type === 'assistant_text') {
              if (!event.text) return null
              return (
                <article key={event.id} className="message message--assistant" aria-label="assistant message">
                  <div className="message__meta">
                    <time dateTime={event.createdAt}>{formatTime(event.createdAt)}</time>
                  </div>
                  <p className="message__content">{event.text}</p>
                </article>
              )
            }

            return null
          })}
        </div>

        {error ? <p className="chat-panel__error">{error}</p> : null}

        <MessageComposer disabled={sending} onSend={handleSend} />
      </section>
    </main>
  )
}
