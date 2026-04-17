import { useState, useRef, useEffect, useCallback } from 'react'
import { ChevronDown, Plus, X, Loader2 } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { WorkspaceMeta } from '@/types/protocol/loom'

export interface WorkspaceSelectorProps {
  workspaces: WorkspaceMeta[]
  activeWorkspaceId: string | null
  loading?: boolean
  error?: string | null
  onSelect: (id: string) => void
  onCreate: (name?: string) => Promise<string | null>
  onRefresh: () => void
}

export function WorkspaceSelector({
  workspaces,
  activeWorkspaceId,
  loading = false,
  error = null,
  onSelect,
  onCreate,
  onRefresh,
}: WorkspaceSelectorProps) {
  const [open, setOpen] = useState(false)
  const [creating, setCreating] = useState(false)
  const [newName, setNewName] = useState('')
  const dropdownRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  const activeWorkspace = workspaces.find(w => w.id === activeWorkspaceId)
  const label = activeWorkspace?.name || activeWorkspaceId || '选择工作空间'

  useEffect(() => {
    if (!open) return
    const handleClickOutside = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [open])

  useEffect(() => {
    if (creating && inputRef.current) {
      inputRef.current.focus()
    }
  }, [creating])

  const handleSelect = useCallback((id: string) => {
    onSelect(id)
    setOpen(false)
    setCreating(false)
  }, [onSelect])

  const handleCreate = useCallback(async () => {
    const name = newName.trim() || undefined
    const id = await onCreate(name)
    if (id) {
      onSelect(id)
      setOpen(false)
      setCreating(false)
      setNewName('')
    }
  }, [newName, onCreate, onSelect])

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      handleCreate()
    } else if (e.key === 'Escape') {
      setCreating(false)
      setNewName('')
    }
  }, [handleCreate])

  return (
    <div className="relative" ref={dropdownRef}>
      <button
        data-testid="workspace-selector"
        onClick={() => setOpen(!open)}
        className="flex items-center gap-2 px-3 py-1.5 text-sm rounded-md hover:bg-accent transition-colors"
        disabled={loading}
      >
        <span className={cn(
          'size-2 shrink-0 rounded-full',
          activeWorkspaceId ? 'bg-emerald-500' : 'bg-muted-foreground/30',
        )} />
        <span className="truncate flex-1 text-left" data-testid="selected-workspace-name">{label}</span>
        <ChevronDown className={cn('size-3.5 shrink-0 transition-transform', open && 'rotate-180')} />
      </button>

      {open && (
        <div
          data-testid="workspace-selector-dropdown"
          className={cn(
            'absolute top-full left-0 right-0 z-50',
            'border-b border-border bg-background shadow-lg',
        )}>
          <div className="px-3 py-2 border-b border-border/60">
            <div className="flex items-center justify-between">
              <span className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">
                工作空间
              </span>
              {loading && <Loader2 className="size-3.5 text-muted-foreground animate-spin" />}
            </div>
            {error && (
              <p className="text-xs text-destructive mt-1">{error}</p>
            )}
          </div>

          <div className="max-h-48 overflow-y-auto py-1">
            {workspaces.length === 0 && !creating && (
              <p className="px-3 py-4 text-center text-xs text-muted-foreground">
                暂无工作空间
              </p>
            )}

            {workspaces.map(ws => (
              <button
                key={ws.id}
                type="button"
                data-testid={`workspace-item-${ws.id}`}
                onClick={() => handleSelect(ws.id)}
                className={cn(
                  'w-full text-left px-3 py-2 text-sm flex items-center gap-2',
                  'hover:bg-muted/60 transition-colors',
                  ws.id === activeWorkspaceId && 'bg-muted/80 font-medium',
                )}
              >
                <span className={cn(
                  'size-1.5 rounded-full shrink-0',
                  ws.id === activeWorkspaceId ? 'bg-emerald-500' : 'bg-muted-foreground/40',
                )} />
                <span className="truncate flex-1">{ws.name || ws.id.slice(0, 8)}</span>
                <span className="text-[0.65rem] text-muted-foreground tabular-nums shrink-0">
                  {new Date(ws.created_at_ms).toLocaleDateString()}
                </span>
              </button>
            ))}

            {creating && (
              <div className="px-3 py-2 flex items-center gap-2">
                <input
                  ref={inputRef}
                  value={newName}
                  onChange={e => setNewName(e.target.value)}
                  onKeyDown={handleKeyDown}
                  data-testid="workspace-create-input"
                  placeholder="工作空间名称"
                  className={cn(
                    'flex-1 min-w-0 px-2 py-1 text-sm rounded-md',
                    'border border-border bg-background',
                    'focus:outline-none focus:ring-1 focus:ring-ring',
                  )}
                />
                <button
                  type="button"
                  onClick={() => { setCreating(false); setNewName('') }}
                  className="size-6 flex items-center justify-center rounded hover:bg-muted/60 text-muted-foreground shrink-0"
                >
                  <X className="size-3" />
                </button>
              </div>
            )}
          </div>

          <div className="border-t border-border/60 px-2 py-1.5">
            {!creating ? (
              <button
                type="button"
                data-testid="workspace-create-btn"
                onClick={() => setCreating(true)}
                className={cn(
                  'w-full flex items-center gap-2 px-2 py-1.5 rounded-md text-sm',
                  'text-muted-foreground hover:text-foreground hover:bg-muted/60 transition-colors',
                )}
              >
                <Plus className="size-3.5" />
                新建工作空间
              </button>
            ) : (
              <button
                type="button"
                data-testid="workspace-create-confirm"
                onClick={handleCreate}
                disabled={loading}
                className={cn(
                  'w-full flex items-center justify-center gap-2 px-2 py-1.5 rounded-md text-sm',
                  'bg-primary text-primary-foreground hover:bg-primary/90 transition-colors',
                  'disabled:opacity-50',
                )}
              >
                {loading ? <Loader2 className="size-3.5 animate-spin" /> : <Plus className="size-3.5" />}
                创建
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  )
}