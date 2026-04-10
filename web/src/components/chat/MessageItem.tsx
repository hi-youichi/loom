import { memo } from 'react'
import type { UIMessageItemProps, UIMessageContent } from '../../types/ui/message'
import { MarkdownContent } from './MarkdownContent'
import { ToolCard } from '../ToolCard'
import type { ToolBlock } from '../../types/chat'

interface MessageItemExtraProps {
  streaming?: boolean
}

function uiStatusToBlockStatus(s: 'pending' | 'running' | 'success' | 'error'): ToolBlock['status'] {
  if (s === 'pending') return 'queued'
  if (s === 'running') return 'running'
  if (s === 'success') return 'done'
  if (s === 'error') return 'error'
  return 'queued'
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
      const tool: ToolBlock = {
        id: item.id,
        type: 'tool',
        callId: item.id,
        name: item.name,
        status: uiStatusToBlockStatus(item.status),
        argumentsText: item.argumentsText,
        outputText: item.outputText,
        resultText: item.resultText,
        isError: item.isError,
      }
      return (
        <div key={index} className="message__tool">
          <ToolCard tool={tool} />
        </div>
      )
    }

    return null
  }

  return (
    <article
      className={`message message--${sender} ${className || ''}`}
      data-message-id={id}
      aria-label={`${sender === 'user' ? 'User' : 'Assistant'} message`}
    >
      <div className="message__content">
        {content.map((item, index) => renderContent(item, index))}
      </div>

      {onRetry && sender === 'user' && (
        <button
          className="message__retry"
          onClick={onRetry}
          aria-label="Retry"
          type="button"
        >
          Retry
        </button>
      )}
    </article>
  )
})
