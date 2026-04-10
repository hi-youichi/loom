import { useState, useMemo } from 'react'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from './ui/popover'
import { cn } from '../lib/utils'
import { useModels, type Model } from '../hooks/useModels'

type ModelOption = {
  value: string
  label: string
}

const DEFAULT_MODELS: ModelOption[] = [
  { value: 'claude-3-5-sonnet', label: 'Claude 3.5 Sonnet' },
  { value: 'claude-3-opus', label: 'Claude 3 Opus' },
  { value: 'claude-3-haiku', label: 'Claude 3 Haiku' },
  { value: 'gpt-4', label: 'GPT-4' },
  { value: 'gpt-3.5-turbo', label: 'GPT-3.5 Turbo' },
]

type ModelSelectorProps = {
  value?: string
  onChange?: (model: string) => void
  disabled?: boolean
  className?: string
  fetchModels?: boolean
}

export function ModelSelector({
  value = DEFAULT_MODELS[0].value,
  onChange,
  disabled = false,
  className = '',
  fetchModels = true,
}: ModelSelectorProps) {
  const [isOpen, setIsOpen] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const { models, loading } = useModels()

  // Convert API models to ModelOption format
  const modelOptions = useMemo(() => {
    if (!fetchModels || models.length === 0) {
      return DEFAULT_MODELS
    }
    
    return models.map((model: Model) => ({
      value: model.id,
      label: model.name
    }))
  }, [fetchModels, models])

  const selectedModel = modelOptions.find(m => m.value === value) || modelOptions[0]

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
      <PopoverTrigger>
        <button
          type="button"
          disabled={disabled}
          className={cn(
            "w-40 text-left font-normal",
            "inline-flex items-center justify-between rounded-md border border-border bg-background px-2 py-1.5 text-sm transition-colors",
            "hover:bg-accent hover:text-accent-foreground",
            "focus-visible:outline-none focus-visible:border-ring",
            "disabled:pointer-events-none disabled:opacity-50",
            className
          )}
        >
          <span className="truncate">{selectedModel.label}</span>
          {loading && <span className="ml-2 text-xs text-muted-foreground">...</span>}
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
        </button>
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
            {loading ? (
              <div className="px-2 py-1.5 text-sm text-muted-foreground">
                加载模型列表...
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