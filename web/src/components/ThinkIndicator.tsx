import { useEffect, useRef, useState } from 'react'

const thinkLines = [
  'Understanding the request.',
  'Reviewing the conversation context.',
  'Preparing a concise response.',
  'Checking available conversation state.',
  'Identifying the relevant UI constraints.',
  'Comparing possible interaction patterns.',
  'Selecting the simplest implementation path.',
  'Refining the output structure.',
  'Verifying consistency with the current layout.',
  'Finalizing the response.',
]

const thinkText = thinkLines.join('\n')

export function ThinkIndicator() {
  const [visibleText, setVisibleText] = useState('')
  const textRef = useRef<HTMLParagraphElement | null>(null)

  useEffect(() => {
    let index = 0
    const timer = window.setInterval(() => {
      index += 1
      setVisibleText(thinkText.slice(0, index))

      if (index >= thinkText.length) {
        window.clearInterval(timer)
      }
    }, 28)

    return () => window.clearInterval(timer)
  }, [])

  useEffect(() => {
    if (textRef.current) {
      textRef.current.scrollTop = textRef.current.scrollHeight
    }
  }, [visibleText])

  const isTyping = visibleText.length < thinkText.length

  return (
    <section className="think-indicator" aria-label="Agent thinking">
      <div className="think-indicator__header">
        <span
          className={`think-indicator__marquee${isTyping ? '' : ' think-indicator__marquee--idle'}`}
          data-text="THINKING"
          aria-label="Thinking"
        >
          THINKING
        </span>
      </div>

      <p ref={textRef} className="think-indicator__typing-text" aria-label={thinkText}>
        <span>{visibleText}</span>
        {isTyping ? <span className="think-indicator__caret" aria-hidden="true" /> : null}
      </p>
    </section>
  )
}
