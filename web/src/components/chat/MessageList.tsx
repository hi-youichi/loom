import { useEffect, useRef, useCallback } from 'react'
import { memo } from 'react'
import type { UIMessageItemProps } from '../../types/ui/message'
import { MessageItem } from './MessageItem'

interface MessageListProps {
  messages: UIMessageItemProps[]
  streaming?: boolean
  className?: string
}

function getMessagesHash(messages: UIMessageItemProps[]): string {
  // Create a hash of the messages to detect any changes
  // Only check structure changes, not content details
  return messages.map(m => 
    `${m.id}-${m.sender}-${m.content.length}-${m.content.map(c => `${c.type}-${c.type === 'text' ? c.text.length : 'tool'}`).join(',')}`
  ).join('|')
}

export const MessageList = memo(function MessageList({ 
  messages, 
  streaming,
  className = '' 
}: MessageListProps) {
  const listRef = useRef<HTMLDivElement>(null)
  const userScrolledRef = useRef(false)
  const lastMessagesHashRef = useRef('')
  const isAutoScrollingRef = useRef(false)

  const handleScroll = useCallback(() => {
    // Don't track scroll while we're auto-scrolling
    if (isAutoScrollingRef.current) return

    const el = listRef.current
    if (!el) return
    
    const threshold = 100 // Increased threshold for better UX
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight
    
    // User is manually scrolling up
    if (distanceFromBottom > threshold) {
      userScrolledRef.current = true
    } else {
      // User scrolled back to bottom, re-enable auto-scroll
      userScrolledRef.current = false
    }
  }, [])

  useEffect(() => {
    const el = listRef.current
    if (!el) return

    const currentHash = getMessagesHash(messages)
    
    // Only scroll if messages changed
    if (currentHash === lastMessagesHashRef.current) return
    
    lastMessagesHashRef.current = currentHash

    // Don't auto-scroll if user manually scrolled up (unless streaming)
    if (userScrolledRef.current && !streaming) return

    // Auto-scroll to bottom
    isAutoScrollingRef.current = true
    el.scrollTop = el.scrollHeight
    
    // Reset auto-scrolling flag after a brief delay
    setTimeout(() => {
      isAutoScrollingRef.current = false
    }, 50)
  }, [messages, streaming])

  // Reset user scroll state when new messages arrive during streaming
  useEffect(() => {
    if (streaming) {
      userScrolledRef.current = false
    }
  }, [streaming, messages.length])

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
      
      {/* Scroll to bottom button when user has scrolled up */}
      {userScrolledRef.current && !streaming && (
        <button
          className="message-list__scroll-to-bottom"
          onClick={() => {
            const el = listRef.current
            if (el) {
              el.scrollTop = el.scrollHeight
              userScrolledRef.current = false
            }
          }}
          aria-label="滚动到底部"
          type="button"
        >
          ↓ 新消息
        </button>
      )}
    </div>
  )
})
