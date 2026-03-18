import { useEffect, useRef } from 'react'
import { memo } from 'react'
import type { UIMessageItemProps } from '../../types/ui/message'
import { MessageItem } from './MessageItem'

interface MessageListProps {
  messages: UIMessageItemProps[]
  className?: string
}

export const MessageList = memo(function MessageList({ 
  messages, 
  className = '' 
}: MessageListProps) {
  const listRef = useRef<HTMLDivElement>(null)

  // 自动滚动到底部
  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight
    }
  }, [messages.length])

  return (
    <div 
      ref={listRef}
      className={`message-list ${className}`}
      role="log" 
      aria-live="polite"
      aria-label="聊天消息"
    >
      {messages.map(message => (
        <MessageItem key={message.id} {...message} />
      ))}
    </div>
  )
})
