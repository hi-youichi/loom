import { useEffect, useRef } from 'react'

type ThinkIndicatorProps = {
  lines: string[]
  active: boolean
}

export function ThinkIndicator({ lines, active }: ThinkIndicatorProps) {
  const textRef = useRef<HTMLParagraphElement | null>(null)
  const thinkText = lines.length > 0 ? lines.join('\n') : 'Awaiting first stream event.'

  useEffect(() => {
    if (textRef.current) {
      textRef.current.scrollTop = textRef.current.scrollHeight
    }
  }, [thinkText])

  return (
    <section className="think-indicator" aria-label="Agent thinking">
      <div className="think-indicator__header">
        <span
          className={`think-indicator__marquee${active ? '' : ' think-indicator__marquee--idle'}`}
          data-text="THINKING"
          aria-label="Thinking"
        >
          THINKING
        </span>
      </div>

      <p ref={textRef} className="think-indicator__typing-text" aria-label={thinkText}>
        <span>{thinkText}</span>
        {active ? <span className="think-indicator__caret" aria-hidden="true" /> : null}
      </p>
    </section>
  )
}
