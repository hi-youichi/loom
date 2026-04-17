import type { UITextContent } from '../../types/ui/message'
import { MarkdownContent } from './MarkdownContent'

interface TextMessageProps {
  content: UITextContent
  className?: string
}

export function TextMessage({ content, className }: TextMessageProps) {
  return (
    <div className={`text-message ${className || ''}`}>
      <MarkdownContent text={content.text} />
    </div>
  )
}
