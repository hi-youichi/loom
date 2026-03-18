import type { UITextContent } from '../../types/ui/message'

interface TextMessageProps {
  content: UITextContent
  className?: string
}

export function TextMessage({ content, className }: TextMessageProps) {
  return (
    <div className={`text-message ${className || ''}`}>
      <p className="text-message__content">
        {content.text}
      </p>
    </div>
  )
}
