import { useState, useMemo } from 'react'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from './ui/popover'
import { cn } from '../lib/utils'
import { useModels, type Model } from '../hooks/useModels'

type ModelSelectorProps = {
  value?: string
  onChange?: (model: string) => void
  disabled?: boolean
  className?: string
}

function LoadingIcon({ className }: { className?: string }) {
  return (
    <svg
      className={cn("animate-spin", className)}
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
    >
      <circle
        className="opacity-25"
        cx="12"
        cy="12"
        r="10"
        stroke="currentColor"
        strokeWidth="4"
      />
      <path
        className="opacity-75"
        fill="currentColor"
        d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
      />
    </svg>
  )
}

export function ModelSelector({
  value,
  onChange,
  disabled = false,
  className = '',
}: ModelSelectorProps) {
  const [isOpen, setIsOpen] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const { models, loading } = useModels()

  const modelOptions = useMemo(() => {
    return models.map((model: Model) => ({
      value: model.id,
      label: model.name
    }))
  }, [models])

  const selectedModel = modelOptions.find(m => m.value === value)

  const filteredModels = useMemo(() => {
    if (!searchQuery.trim()) return modelOptions
    const query = searchQuery.toLowerCase()
    return modelOptions.filter(model =>
      model.label.toLowerCase().includes(query) ||
      model.value.toLowerCase().includes(query)
    )
  }, [searchQuery, modelOptions])

  const handleSelect = (modelValue: string) => {
    onChange?.(modelValue)
    setIsOpen(false)
    setSearchQuery('')
  }

  const handleSearchChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setSearchQuery(e.target.value)
  }

  return (
    <Popover open={isOpen} onOpenChange={setIsOpen}>
      <PopoverTrigger
        render={
          <span
            className={cn(
              "w-40 text-left font-normal cursor-pointer",
              "inline-flex items-center justify-between rounded-md border border-border bg-background px-2 py-1.5 text-sm transition-colors",
              "hover:bg-accent hover:text-accent-foreground",
              "focus-visible:outline-none focus-visible:border-ring",
              "disabled:pointer-events-none disabled:opacity-50",
              disabled && "pointer-events-none opacity-50",
              className
            )}
          />
        }
      >
        {loading && !selectedModel ? (
          <span className="flex items-center gap-1.5 text-muted-foreground">
            <LoadingIcon className="h-3.5 w-3.5" />
            <span>加载中</span>
          </span>
        ) : (
          <span className="truncate">{selectedModel?.label || value}</span>
        )}
        <svg
          className="ml-1 h-3.5 w-3.5 shrink-0 opacity-50"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M19 9l-7 7-7-7"
          />
        </svg>
      </PopoverTrigger>
      <PopoverContent className="w-40 p-1" align="start" side="bottom">
        <div className="space-y-1">
          <input
            type="text"
            placeholder="搜索模型..."
            value={searchQuery}
            onChange={handleSearchChange}
            className="w-full rounded-md border border-border bg-background px-2 py-1.5 text-sm outline-none focus:border-ring"
            autoFocus
          />
          <div className="max-h-48 overflow-y-auto">
            {loading && modelOptions.length === 0 ? (
              <div className="flex items-center justify-center gap-1.5 px-2 py-1.5 text-sm text-muted-foreground">
                <LoadingIcon className="h-3.5 w-3.5" />
                <span>加载模型列表...</span>
              </div>
            ) : filteredModels.length > 0 ? (
              filteredModels.map((model) => (
                <button
                  key={model.value}
                  onClick={() => handleSelect(model.value)}
                  className={cn(
                    "w-full text-left px-2 py-1.5 text-sm rounded-md transition-colors",
                    "hover:bg-accent hover:text-accent-foreground",
                    "focus:outline-none focus:border-ring",
                    model.value === value && "bg-accent text-accent-foreground"
                  )}
                >
                  <span className="block truncate">{model.label}</span>
                </button>
              ))
            ) : (
              <div className="px-2 py-1.5 text-sm text-muted-foreground">
                未找到匹配的模型
              </div>
            )}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}
