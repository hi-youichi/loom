import { Fragment, useState } from 'react'

import { MessageComposer } from '../components/MessageComposer'
import { ThinkIndicator } from '../components/ThinkIndicator'
import { sendMessage } from '../services/chat'
import type { Message } from '../types/chat'

const initialMessages: Message[] = [
  {
    id: 'user-demo',
    role: 'user',
    content: 'Can you help me refine the message composer layout?',
    createdAt: new Date().toISOString(),
  },
  {
    id: 'welcome',
    role: 'assistant',
    content:
      'Absolutely. Start with a clean single-column layout, smaller message text, and a compact send button.',
    createdAt: new Date().toISOString(),
  },
]

function formatTime(timestamp: string) {
  return new Intl.DateTimeFormat('en', {
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(timestamp))
}

export function ChatPage() {
  const [messages, setMessages] = useState<Message[]>(initialMessages)
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleSend = async (text: string) => {
    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      content: text,
      createdAt: new Date().toISOString(),
    }

    setMessages((current) => [...current, userMessage])
    setSending(true)
    setError(null)

    try {
      const reply = await sendMessage(text)

      setMessages((current) => [
        ...current,
        {
          id: crypto.randomUUID(),
          role: 'assistant',
          content: reply.content,
          createdAt: new Date().toISOString(),
        },
      ])
    } catch {
      setError('Request failed. Try again, or send a message without the word "error".')
    } finally {
      setSending(false)
    }
  }

  return (
    <main className="shell">
      <section className="chat-panel" aria-label="Chat demo">
        <div className="message-list" role="log" aria-live="polite">
          {messages.map((message) => (
            <Fragment key={message.id}>
              <article
                className={`message message--${message.role}`}
                aria-label={`${message.role} message`}
              >
                <div className="message__meta">
                  <time dateTime={message.createdAt}>{formatTime(message.createdAt)}</time>
                </div>
                <p className="message__content">{message.content}</p>
              </article>

              {message.id === 'user-demo' ? <ThinkIndicator /> : null}
            </Fragment>
          ))}
        </div>

        {error ? <p className="chat-panel__error">{error}</p> : null}

        <MessageComposer disabled={sending} onSend={handleSend} />
      </section>
    </main>
  )
}
