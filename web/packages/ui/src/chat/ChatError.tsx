import { useCallback } from 'react'

interface ChatErrorProps {
  error: string
  onDismiss?: () => void
}

export function ChatError({ error, onDismiss }: ChatErrorProps) {
  const handleDismiss = useCallback(() => {
    onDismiss?.()
  }, [onDismiss])

  return (
    <div className="flex items-start gap-2 mx-3 my-2 px-3 py-2 rounded-md bg-destructive/10 border border-destructive/20 text-sm text-destructive">
      <span className="shrink-0 mt-0.5">⚠</span>
      <span className="flex-1 min-w-0 break-words">{error}</span>
      {onDismiss && (
        <button
          type="button"
          onClick={handleDismiss}
          className="shrink-0 text-destructive/60 hover:text-destructive transition-colors"
          aria-label="Dismiss error"
        >
          ✕
        </button>
      )}
    </div>
  )
}
