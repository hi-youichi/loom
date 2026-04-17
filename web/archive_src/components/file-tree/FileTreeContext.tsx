/* eslint-disable react-refresh/only-export-components */
import { createContext, useState, useCallback, useMemo } from 'react'
import type { FileNode, FileTreeContextValue } from './types'

export const FileTreeContext = createContext<FileTreeContextValue | null>(null)

interface FileTreeProviderProps {
  children: React.ReactNode
  selectedId?: string | null
  onSelect?: (node: FileNode) => void
  initialExpandedIds?: string[]
}

export function FileTreeProvider({
  children,
  selectedId: controlledSelectedId,
  onSelect,
  initialExpandedIds = [],
}: FileTreeProviderProps) {
  const [internalSelectedId, setInternalSelectedId] = useState<string | null>(null)
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set(initialExpandedIds))
  const [searchQuery, setSearchQuery] = useState('')

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

  const value = useMemo(
    () => ({
      selectedId,
      expandedIds,
      searchQuery,
      onSelect: handleSelect,
      onToggle: handleToggle,
      setSearchQuery,
    }),
    [selectedId, expandedIds, searchQuery, handleSelect, handleToggle]
  )

  return <FileTreeContext.Provider value={value}>{children}</FileTreeContext.Provider>
}
