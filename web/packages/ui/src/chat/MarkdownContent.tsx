import { useMemo } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'

interface MarkdownContentProps {
  text: string
  streaming?: boolean
  className?: string
}

function preprocessIncompleteMarkdown(text: string): string {
  let result = text
  const fenceCount = (result.match(/```/g) || []).length
  if (fenceCount % 2 !== 0) {
    result += '\n```'
  }
  return result
}

export function MarkdownContent({ text, streaming, className }: MarkdownContentProps) {
  const processed = useMemo(() => {
    if (!text) return ''
    if (streaming) return preprocessIncompleteMarkdown(text)
    return text
  }, [text, streaming])

  if (!processed) return null

  return (
    <div className={`markdown-body ${className || ''}`}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
        {processed}
      </ReactMarkdown>
    </div>
  )
}
