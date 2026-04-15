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
  className,
}: ModelSelectorProps) {
  const [searchQuery, setSearchQuery] = useState('')
  const [open, setOpen] = useState(false)
  const { models, loading } = useModels()

  const groups = useMemo(() => {
    const map = new Map<string, Model[]>()
    for (const model of models) {
      const provider = model.provider || 'Other'
      if (!map.has(provider)) map.set(provider, [])
      map.get(provider)!.push(model)
    }
    return map
  }, [models])

  const selectedModel = models.find(m => m.id === value)

  const filteredGroups = useMemo(() => {
    if (!searchQuery.trim()) return groups
    const query = searchQuery.toLowerCase()
    const filtered = new Map<string, Model[]>()
    for (const [provider, list] of groups) {
      if (provider.toLowerCase().includes(query)) {
        filtered.set(provider, list)
        continue
      }
      const matches = list.filter(m => m.name.toLowerCase().includes(query))
      if (matches.length > 0) filtered.set(provider, matches)
    }
    return filtered
  }, [groups, searchQuery])

  return (
    <div className={cn("model-selector", className)}>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger disabled={disabled}>
          <div
            className={cn(
              "model-selector__trigger",
              "flex items-center gap-2 px-3 border border-border bg-background",
              "hover:bg-accent/50",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              "disabled:opacity-50 disabled:cursor-not-allowed",
              "transition-colors cursor-pointer"
            )}
          >
            {loading && !selectedModel ? (
              <LoadingIcon className="h-3.5 w-3.5" />
            ) : (
              <span className="truncate text-sm">
                {selectedModel ? selectedModel.name : 'Select model...'}
              </span>
            )}
            <svg
              className={cn(
                "ml-auto h-4 w-4 shrink-0 text-muted-foreground transition-transform",
                open && "rotate-180"
              )}
              xmlns="http://www.w3.org/2000/svg"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <polyline points="6 9 12 15 18 9" />
            </svg>
          </div>
        </PopoverTrigger>
        <PopoverContent align="start" sideOffset={4} className="w-[280px]">
          <div className="model-selector__content">
            <div className="px-1 pb-1">
              <input
                className="w-full rounded-md border border-border bg-background px-2 py-1.5 text-sm outline-none focus:outline-none"
                placeholder="Search models..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
              />
            </div>

            <div className="overflow-y-auto max-h-[260px]">
              {filteredGroups.size > 0 ? (
                [...filteredGroups.entries()].map(([provider, list]) => (
                  <div key={provider}>
                    <div className="model-selector__group-title">
                      {provider}
                    </div>
                    {list.map((model) => (
                      <button
                        key={model.id}
                        type="button"
                        className={cn(
                          "w-full text-left px-3 py-1.5 text-sm rounded-sm",
                          "hover:bg-accent hover:text-accent-foreground",
                          "focus:outline-none",
                          model.id === value && "bg-accent text-accent-foreground"
                        )}
                        onClick={() => {
                          if (import.meta.env.DEV) {
                            console.log('🎯 Model selected:', model.id, model.name);
                          }
                          onChange?.(model.id)
                          setOpen(false)
                          setSearchQuery('')
                        }}
                      >
                        <span className="block truncate">{model.name}</span>
                      </button>
                    ))}
                  </div>
                ))
              ) : (
                <div className="px-2 py-1.5 text-sm text-muted-foreground">
                  No models found
                </div>
              )}
            </div>
          </div>
        </PopoverContent>
      </Popover>
    </div>
  )
}
