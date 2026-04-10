import { useLayoutEffect, useRef, useState } from 'react'

type MessageComposerProps = {
  disabled?: boolean
  onSend: (text: string) => Promise<void>
}

function resizeTextarea(element: HTMLTextAreaElement) {
  const currentHeight = element.getBoundingClientRect().height

  element.style.height = 'auto'
  const nextHeight = Math.min(element.scrollHeight, 220)

  if (Math.abs(currentHeight - nextHeight) < 1) {
    element.style.height = `${nextHeight}px`
    return
  }

  element.style.height = `${currentHeight}px`

  requestAnimationFrame(() => {
    element.style.height = `${nextHeight}px`
  })
}

export function MessageComposer({
  disabled = false,
  onSend,
}: MessageComposerProps) {
  const [value, setValue] = useState('')
  const textareaRef = useRef<HTMLTextAreaElement | null>(null)

  useLayoutEffect(() => {
    if (textareaRef.current) {
      resizeTextarea(textareaRef.current)
    }
  }, [value])

  const handleSubmit = async () => {
    const text = value.trim()
    if (!text || disabled) {
      return
    }

    setValue('')
    try {
      await onSend(text)
    } catch {
      setValue(text)
    }
  }

  return (
    <form
      className="composer"
      onSubmit={(event) => {
        event.preventDefault()
        void handleSubmit()
      }}
    >
      <label className="sr-only" htmlFor="message-input">
        Message
      </label>
      <textarea
        id="message-input"
        ref={textareaRef}
        className="composer__input"
        value={value}
        rows={1}
        placeholder="Write a message. Press Enter to send."
        disabled={disabled}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === 'Enter' && !event.shiftKey) {
            event.preventDefault()
            void handleSubmit()
          }
        }}
      />
      <button
        className="composer__button"
        type="submit"
        aria-label={disabled ? 'Sending message' : 'Send message'}
        disabled={disabled || !value.trim()}
      >
        <svg
          className="composer__button-icon"
          viewBox="0 0 24 24"
          fill="none"
          xmlns="http://www.w3.org/2000/svg"
          aria-hidden="true"
        >
          <path
            d="M12 19V6M7 11L12 6L17 11"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </button>
    </form>
  )
}
