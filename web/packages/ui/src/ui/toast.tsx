import { createContext, useContext, useState, useCallback, useMemo } from 'react'
import { Toast as ToastPrimitive } from 'radix-ui'
import { cn } from '../lib/utils'
import { X } from 'lucide-react'

type ToastData = {
  id: string
  title: string
  description?: string
  variant?: 'default' | 'destructive'
}

type ToastContextValue = {
  toasts: ToastData[]
  addToast: (toast: Omit<ToastData, 'id'>) => void
  removeToast: (id: string) => void
}

const ToastContext = createContext<ToastContextValue | null>(null)

export function useToast() {
  const ctx = useContext(ToastContext)
  if (!ctx) throw new Error('useToast must be used within ToastProvider')
  return ctx
}

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = useState<ToastData[]>([])

  const addToast = useCallback((toast: Omit<ToastData, 'id'>) => {
    const id = `toast-${Date.now()}`
    setToasts(prev => [...prev, { ...toast, id }])
    setTimeout(() => {
      setToasts(prev => prev.filter(t => t.id !== id))
    }, 3000)
  }, [])

  const removeToast = useCallback((id: string) => {
    setToasts(prev => prev.filter(t => t.id !== id))
  }, [])

  const value = useMemo(() => ({ toasts, addToast, removeToast }), [toasts, addToast, removeToast])

  return (
    <ToastContext.Provider value={value}>
      {children}
      <ToastViewport toasts={toasts} onRemove={removeToast} />
    </ToastContext.Provider>
  )
}

function ToastViewport({ toasts, onRemove }: { toasts: ToastData[]; onRemove: (id: string) => void }) {
  if (toasts.length === 0) return null

  return (
    <div
      data-slot="toast-viewport"
      className="fixed bottom-4 right-4 z-[100] flex flex-col gap-2 w-[320px]"
    >
      {toasts.map(toast => (
        <div
          key={toast.id}
          data-testid={`toast-${toast.id}`}
          role="status"
          className={cn(
            'flex items-start gap-3 rounded-md border p-3 shadow-lg transition-all',
            'bg-background text-foreground border-border',
            'animate-in slide-in-from-bottom-4 fade-in-0',
            toast.variant === 'destructive' && 'border-destructive/50 text-destructive'
          )}
        >
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium">{toast.title}</p>
            {toast.description && (
              <p className="text-xs text-muted-foreground mt-0.5">{toast.description}</p>
            )}
          </div>
          <button
            type="button"
            onClick={() => onRemove(toast.id)}
            className="shrink-0 rounded-md p-0.5 hover:bg-muted transition-colors"
          >
            <X className="size-3.5 text-muted-foreground" />
          </button>
        </div>
      ))}
    </div>
  )
}
