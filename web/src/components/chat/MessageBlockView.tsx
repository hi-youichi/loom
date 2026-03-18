import { memo } from 'react'
import type { UIMessageContent } from '../../types/ui/message'
import { isUITextContent, isUIToolContent } from '../../types/ui/message'
import { TextMessage } from './TextMessage'
import { ToolMessage } from './ToolMessage'

interface MessageBlockViewProps {
  content: UIMessageContent
  className?: string
}

/**
 * 消息块视图组件
 * 根据消息块的类型选择正确的组件来渲染
 * 协议无关，只依赖通用UI类型
 */
export const MessageBlockView = memo(function MessageBlockView({
  content,
  className,
}: MessageBlockViewProps) {
  if (isUITextContent(content)) {
    return <TextMessage content={content} className={className} />
  }

  if (isUIToolContent(content)) {
    return <ToolMessage content={content} className={className} />
  }

  // 未知类型的后备处理
  console.warn('Unknown message content type:', content)
  return null
})
