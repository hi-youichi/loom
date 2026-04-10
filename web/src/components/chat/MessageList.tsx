import { useEffect, useRef, useCallback } from 'react'
import { memo } from 'react'
import type { UIMessageItemProps } from '../../types/ui/message'
import { MessageItem } from './MessageItem'

interface MessageListProps {
  messages: UIMessageItemProps[]
  streaming?: boolean
  className?: string
}

function getMessagesTextLength(messages: UIMessageItemProps[]): number {
  let len = 0
  for (const m of messages) {
    for (const c of m.content) {
      if (c.type === 'text') len += c.text.length
    }
  }
  return len
}

export const MessageList = memo(function MessageList({ 
  messages, 
  streaming,
  className = '' 
}: MessageListProps) {
  const listRef = useRef<HTMLDivElement>(null)
  const userScrolledRef = useRef(false)
  const lastTextLenRef = useRef(0)

  const handleScroll = useCallback(() => {
    const el = listRef.current
    if (!el) return
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40
    userScrolledRef.current = !atBottom
  }, [])

  useEffect(() => {
    const textLen = getMessagesTextLength(messages)
    if (textLen === lastTextLenRef.current) return
    lastTextLenRef.current = textLen

    if (userScrolledRef.current && !streaming) return

    const el = listRef.current
    if (el) {
      el.scrollTop = el.scrollHeight
    }
  })

  useEffect(() => {
    userScrolledRef.current = false
    lastTextLenRef.current = 0
  }, [messages.length])

  return (
    <div 
      ref={listRef}
      onScroll={handleScroll}
      className={`message-list text-sm h-full overflow-y-auto ${className}`}
      role="log" 
      aria-live="polite"
      aria-label="Chat messages"
    >
      {messages.map(message => (
        <MessageItem key={message.id} {...message} streaming={streaming && message.sender === 'assistant'} />
      ))}
      {streaming && (
        <div className="message-list__streaming">
          <span className="streaming-indicator" />
        </div>
      )}
    </div>
  )
})
