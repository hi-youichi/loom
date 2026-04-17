import { useEffect, useRef, useState, useCallback } from 'react'
import { memo } from 'react'
import type { UIMessageItemProps } from '@loom/types'
import { MessageItem } from './MessageItem'

interface MessageListProps {
  messages: UIMessageItemProps[]
  streaming?: boolean
  className?: string
}

export const MessageList = memo(function MessageList({ 
  messages, 
  streaming,
  className = '' 
}: MessageListProps) {
  const listRef = useRef<HTMLDivElement>(null)
  const isAutoScrollingRef = useRef(false)
  const [userScrolledUp, setUserScrolledUp] = useState(false)

  const handleScroll = useCallback(() => {
    if (isAutoScrollingRef.current) return

    const el = listRef.current
    if (!el) return

    const threshold = 100
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight

    setUserScrolledUp(distanceFromBottom > threshold)
  }, [])

  useEffect(() => {
    const el = listRef.current
    if (!el) return

    if (userScrolledUp && !streaming) return

    isAutoScrollingRef.current = true
    el.scrollTop = el.scrollHeight

    setTimeout(() => {
      isAutoScrollingRef.current = false
    }, 50)
  }, [messages, streaming, userScrolledUp])

  useEffect(() => {
    if (streaming) {
      setUserScrolledUp(false)
    }
  }, [streaming, messages.length])

  const scrollToBottom = useCallback(() => {
    const el = listRef.current
    if (el) {
      el.scrollTop = el.scrollHeight
      setUserScrolledUp(false)
    }
  }, [])

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
      
      {userScrolledUp && !streaming && (
        <button
          className="message-list__scroll-to-bottom"
          onClick={scrollToBottom}
          aria-label="Scroll to bottom"
          type="button"
        >
          ↓ 新消息
        </button>
      )}
    </div>
  )
})
