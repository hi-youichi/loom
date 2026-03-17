import { Fragment, useMemo, useState } from 'react'

import { MessageComposer } from '../components/MessageComposer'
import { ThinkIndicator } from '../components/ThinkIndicator'
import { sendMessage } from '../services/chat'
import type { Message } from '../types/chat'

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

export function ChatPage() {
  const threadId = useMemo(() => getOrCreateThreadId(), [])
  const [messages, setMessages] = useState<Message[]>([])
  const [sending, setSending] = useState(false)
  const [pendingAssistantId, setPendingAssistantId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const handleSend = async (text: string) => {
    const createdAt = new Date().toISOString()
    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      content: text,
      createdAt,
    }
    const assistantMessageId = crypto.randomUUID()
    const assistantMessage: Message = {
      id: assistantMessageId,
      role: 'assistant',
      content: '',
      createdAt,
    }

    setMessages((current) => [...current, userMessage, assistantMessage])
    setSending(true)
    setPendingAssistantId(assistantMessageId)
    setError(null)

    try {
      const reply = await sendMessage(text, {
        threadId,
        onChunk: (chunk) => {
          setMessages((current) =>
            current.map((message) =>
              message.id === assistantMessageId
                ? { ...message, content: message.content + chunk }
                : message,
            ),
          )
        },
      })

      setMessages((current) =>
        current.map((message) =>
          message.id === assistantMessageId
            ? { ...message, content: reply.content || message.content }
            : message,
        ),
      )
    } catch (caughtError) {
      const nextError =
        caughtError instanceof Error
          ? caughtError.message
          : 'Request failed. Check whether `loom serve` is running.'

      setMessages((current) =>
        current.filter((message) => message.id !== assistantMessageId),
      )
      setError(nextError)
      throw caughtError
    } finally {
      setSending(false)
      setPendingAssistantId(null)
    }
  }

  return (
    <main className="shell">
      <section className="chat-panel" aria-label="Chat demo">
        <div className="message-list" role="log" aria-live="polite">
          {messages.map((message) => (
            <Fragment key={message.id}>
              {message.content ? (
                <article
                  className={`message message--${message.role}`}
                  aria-label={`${message.role} message`}
                >
                  <div className="message__meta">
                    <time dateTime={message.createdAt}>{formatTime(message.createdAt)}</time>
                  </div>
                  <p className="message__content">{message.content}</p>
                </article>
              ) : null}

              {message.id === pendingAssistantId && !message.content ? <ThinkIndicator /> : null}
            </Fragment>
          ))}
        </div>

        {error ? <p className="chat-panel__error">{error}</p> : null}

        <MessageComposer disabled={sending} onSend={handleSend} />
      </section>
    </main>
  )
}
