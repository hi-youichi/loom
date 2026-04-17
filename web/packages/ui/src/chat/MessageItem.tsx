import { memo } from 'react'
import type { UIMessageItemProps, UITextContent, UIToolContent } from '@graphweave/types'
import { MarkdownContent } from './MarkdownContent'
import { ToolCard } from '../ToolCard'
import type { ToolBlock } from '@graphweave/types'

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
  const textItems = content.filter((item): item is UITextContent => item.type === 'text')
  const toolItems = content.filter((item): item is UIToolContent => item.type === 'tool')

  return (
    <>
      {content.map((item, index) => {
        if (item.type === 'text') {
          return (
            <article
              key={`text-${index}`}
              className={`message message--${sender} ${className || ''}`}
              data-message-id={id}
              aria-label={`${sender === 'user' ? 'User' : 'Assistant'} message`}
            >
              <div className="message__content">
                <div className="message__text">
                  <MarkdownContent text={item.text} streaming={streaming} />
                </div>
              </div>

              {onRetry && sender === 'user' && index === 0 && (
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
        } else if (item.type === 'tool') {
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
          return <ToolCard key={`tool-${index}`} tool={tool} />
        }
        return null
      })}
    </>
  )
})
