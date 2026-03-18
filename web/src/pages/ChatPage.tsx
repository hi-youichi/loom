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

function formatThinkLine(event: Record<string, unknown>) {
  switch (event.type) {
    case 'run_start':
      return `run_start${typeof event.agent === 'string' ? ` · ${event.agent}` : ''}`
    case 'node_enter':
      return `node_enter${typeof event.id === 'string' ? ` · ${event.id}` : ''}`
    case 'node_exit': {
      const node = typeof event.id === 'string' ? ` · ${event.id}` : ''
      const result =
        typeof event.result === 'string' ? event.result : JSON.stringify(event.result)
      return `node_exit${node}${result ? ` · ${result}` : ''}`
    }
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
    case 'usage':
      return typeof event.total_tokens === 'number'
        ? `usage · total tokens ${event.total_tokens}`
        : 'usage'
    case 'message_chunk':
      return null
    default:
      return typeof event.type === 'string' ? event.type : null
  }
}

export function ChatPage() {
  const threadId = useMemo(() => getOrCreateThreadId(), [])
  const [messages, setMessages] = useState<Message[]>([])
  const [sending, setSending] = useState(false)
  const [thinkingByMessageId, setThinkingByMessageId] = useState<Record<string, ThinkingState>>({})
  const [error, setError] = useState<string | null>(null)

  const handleSend = async (text: string) => {
    const createdAt = new Date().toISOString()
    const userMessage = createTextMessage('user', text, createdAt)
    const assistantMessage = createTextMessage('assistant', '', createdAt)
    const assistantMessageId = assistantMessage.id

    setMessages((current) => [...current, userMessage, assistantMessage])
    setSending(true)
    setThinkingByMessageId((current) => ({
      ...current,
      [assistantMessageId]: {
        lines: [],
        active: true,
      },
    }))
    setError(null)

    try {
      const reply = await sendMessage(text, {
        threadId,
        onEvent: (event) => {
          const line = formatThinkLine(event)
          if (!line) {
            return
          }

          setThinkingByMessageId((current) => {
            const existing = current[assistantMessageId] ?? { lines: [], active: true }

            return {
              ...current,
              [assistantMessageId]: {
                ...existing,
                lines: [...existing.lines, line],
              },
            }
          })
        },
        onChunk: (chunk) => {
          setMessages((current) =>
            current.map((message) =>
              message.id === assistantMessageId
                ? appendMessageChunk(message, chunk)
                : message,
            ),
          )
        },
      })

      setMessages((current) =>
        current.map((message) =>
          message.id === assistantMessageId
            ? replaceMessageText(message, reply.content || getMessageText(message))
            : message,
        ),
      )
      setThinkingByMessageId((current) => ({
        ...current,
        [assistantMessageId]: {
          ...(current[assistantMessageId] ?? { lines: [] }),
          active: false,
        },
      }))
    } catch (caughtError) {
      const nextError =
        caughtError instanceof Error
          ? caughtError.message
          : 'Request failed. Check whether `loom serve` is running.'

      setMessages((current) =>
        current.filter((message) => message.id !== assistantMessageId),
      )
      setThinkingByMessageId((current) => {
        const nextState = { ...current }
        delete nextState[assistantMessageId]
        return nextState
      })
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
          {messages.map((message) => {
            const messageText = getMessageText(message)

            return (
              <Fragment key={message.id}>
                {messageText ? (
                  <article
                    className={`message message--${message.role}`}
                    aria-label={`${message.role} message`}
                  >
                    <div className="message__meta">
                      <time dateTime={message.createdAt}>{formatTime(message.createdAt)}</time>
                    </div>
                    <p className="message__content">{messageText}</p>
                  </article>
                ) : null}

                {thinkingByMessageId[message.id] ? (
                  <ThinkIndicator
                    lines={thinkingByMessageId[message.id].lines}
                    active={thinkingByMessageId[message.id].active}
                  />
                ) : null}
              </Fragment>
            )
          })}
        </div>

        {error ? <p className="chat-panel__error">{error}</p> : null}

        <MessageComposer disabled={sending} onSend={handleSend} />
      </section>
    </main>
  )
}
