import { memo } from 'react'
import type { UIMessageItemProps, UIMessageContent } from '../../types/ui/message'
import { MarkdownContent } from './MarkdownContent'

interface MessageItemExtraProps {
  streaming?: boolean
}

export const MessageItem = memo(function MessageItem({
  id,
  sender,
  timestamp: _timestamp,
  content,
  className,
  onRetry,
  streaming,
}: UIMessageItemProps & MessageItemExtraProps) {
  const renderContent = (item: UIMessageContent, index: number) => {
    if (item.type === 'text') {
      return (
        <div key={index} className="message__text">
          <MarkdownContent text={item.text} streaming={streaming} />
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
          {item.resultText && !item.isError && (
            <div className="tool__result">
              <strong>结果:</strong>
              <pre>{item.resultText}</pre>
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
