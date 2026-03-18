import { memo } from 'react'
import type { UIMessageItemProps, UIMessageContent } from '../../types/ui/message'

/**
 * 消息项组件 - 协议无关
 * 只依赖通用UI类型，不依赖任何特定协议
 */
export const MessageItem = memo(function MessageItem({
  id,
  sender,
  timestamp,
  content,
  className,
  onRetry,
}: UIMessageItemProps) {
  const formatTime = (ts: string) => {
    return new Intl.DateTimeFormat('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(ts))
  }

  const renderContent = (item: UIMessageContent, index: number) => {
    if (item.type === 'text') {
      return (
        <div key={index} className="message__text">
          {item.text}
        </div>
      )
    }

    if (item.type === 'tool') {
      return (
        <div key={index} className="message__tool">
          <div className="tool__header">
            <span className="tool__name">{item.name}</span>
            <span className={`tool__status tool__status--${item.status}`}>
              {item.status}
            </span>
          </div>
          {item.argumentsText && (
            <div className="tool__arguments">
              <strong>参数:</strong>
              <pre>{item.argumentsText}</pre>
            </div>
          )}
          {item.outputText && (
            <div className="tool__output">
              <strong>输出:</strong>
              <pre>{item.outputText}</pre>
            </div>
          )}
          {item.isError && (
            <div className="tool__error">
              <strong>错误:</strong>
              <pre>{item.resultText}</pre>
            </div>
          )}
        </div>
      )
    }

    return null
  }

  return (
    <article
      className={`message message--${sender} ${className || ''}`}
      data-message-id={id}
      aria-label={`${sender === 'user' ? '用户' : '助手'}消息`}
    >
      <header className="message__header">
        <span className="message__sender" aria-label="发送者">
          {sender === 'user' ? '用户' : '助手'}
        </span>
        <time
          className="message__time"
          dateTime={timestamp}
          aria-label={`发送时间 ${formatTime(timestamp)}`}
        >
          {formatTime(timestamp)}
        </time>
      </header>

      <div className="message__content">
        {content.map((item, index) => renderContent(item, index))}
      </div>

      {onRetry && sender === 'user' && (
        <button
          className="message__retry"
          onClick={onRetry}
          aria-label="重试发送"
        >
          重试
        </button>
      )}
    </article>
  )
})
