import { createContext, useState, useCallback, useMemo } from 'react'
import type { FileNode, FileTreeContextValue } from './types'

export const FileTreeContext = createContext<FileTreeContextValue | null>(null)

interface FileTreeProviderProps {
  children: React.ReactNode
  selectedId?: string | null
  onSelect?: (node: FileNode) => void
  initialExpandedIds?: string[]
  onRename?: (node: FileNode, newName: string) => void
  onDelete?: (node: FileNode) => void
  onCreateFile?: (parentId: string, name: string) => void
  onCreateFolder?: (parentId: string, name: string) => void
  onRefresh?: () => void
  onCopyPath?: (node: FileNode) => void
}

export function FileTreeProvider({
  children,
  selectedId: controlledSelectedId,
  onSelect,
  initialExpandedIds = [],
  onRename,
  onDelete,
  onCreateFile,
  onCreateFolder,
  onRefresh,
  onCopyPath,
}: FileTreeProviderProps) {
  const [internalSelectedId, setInternalSelectedId] = useState<string | null>(null)
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set(initialExpandedIds))
  const [searchQuery, setSearchQuery] = useState('')
  const [renamingId, setRenamingId] = useState<string | null>(null)
  const [creatingIn, setCreatingIn] = useState<string | null>(null)
  const [creatingType, setCreatingType] = useState<'file' | 'folder' | null>(null)

  const selectedId = controlledSelectedId ?? internalSelectedId

  const handleSelect = useCallback(
    (node: FileNode) => {
      setInternalSelectedId(node.id)
      onSelect?.(node)
    },
    [onSelect]
  )

  const handleToggle = useCallback((id: string) => {
    setExpandedIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }, [])

  const startRename = useCallback((id: string) => {
    setRenamingId(id)
  }, [])

  const cancelRename = useCallback(() => {
    setRenamingId(null)
  }, [])

  const commitRename = useCallback(
    (id: string, newName: string) => {
      setRenamingId(null)
      onRename?.({ id, name: newName } as FileNode, newName)
    },
    [onRename]
  )

  const startCreate = useCallback((parentId: string, type: 'file' | 'folder') => {
    setCreatingIn(parentId)
    setCreatingType(type)
    setExpandedIds((prev) => {
      const next = new Set(prev)
      next.add(parentId)
      return next
    })
  }, [])

  const cancelCreate = useCallback(() => {
    setCreatingIn(null)
    setCreatingType(null)
  }, [])

  const commitCreate = useCallback(
    (parentId: string, type: 'file' | 'folder', name: string) => {
      setCreatingIn(null)
      setCreatingType(null)
      if (type === 'file') {
        onCreateFile?.(parentId, name)
      } else {
        onCreateFolder?.(parentId, name)
      }
    },
    [onCreateFile, onCreateFolder]
  )

  const handleDelete = useCallback(
    (node: FileNode) => {
      onDelete?.(node)
    },
    [onDelete]
  )

  const handleCopyPath = useCallback(
    (node: FileNode) => {
      onCopyPath?.(node)
    },
    [onCopyPath]
  )

  const handleRefresh = useCallback(() => {
    onRefresh?.()
  }, [onRefresh])

  const value = useMemo(
    () => ({
      selectedId,
      expandedIds,
      searchQuery,
      onSelect: handleSelect,
      onToggle: handleToggle,
      setSearchQuery,
      renamingId,
      startRename,
      cancelRename,
      commitRename,
      creatingIn,
      creatingType,
      startCreate,
      cancelCreate,
      commitCreate,
      onDelete: handleDelete,
      onCopyPath: handleCopyPath,
      onRefresh: handleRefresh,
    }),
    [
      selectedId, expandedIds, searchQuery, handleSelect, handleToggle,
      renamingId, startRename, cancelRename, commitRename,
      creatingIn, creatingType, startCreate, cancelCreate, commitCreate,
      handleDelete, handleCopyPath, handleRefresh,
    ]
  )

  return <FileTreeContext.Provider value={value}>{children}</FileTreeContext.Provider>
}
