import { useState, useCallback, useEffect, useRef } from 'react'
import type { FileEntry } from '@loom/protocol'
import * as wsApi from '@loom/service-workspace'

export type FileNode = {
  id: string
  name: string
  type: 'file' | 'folder'
  path: string
  extension?: string
  size?: number
  children?: FileNode[]
  loaded?: boolean
}

function entriesToNodes(entries: FileEntry[], parentPath: string): FileNode[] {
  return entries.map((entry) => ({
    id: `${parentPath}/${entry.name}`,
    name: entry.name,
    type: entry.kind,
    path: entry.path,
    extension: entry.extension,
    size: entry.size,
    children: entry.kind === 'folder' ? [] : undefined,
    loaded: entry.kind === 'folder' ? false : undefined,
  }))
}

export function useWorkspaceFiles(workspaceId: string | null | undefined) {
  const [rootFiles, setRootFiles] = useState<FileNode[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const cacheRef = useRef<Map<string, FileNode[]>>(new Map())

  const loadDirectory = useCallback(
    async (path: string): Promise<FileNode[]> => {
      if (!workspaceId) return []
      const cached = cacheRef.current.get(path)
      if (cached) return cached

      const entries = await wsApi.listFiles(workspaceId, path || undefined)
      const nodes = entriesToNodes(entries, path)
      cacheRef.current.set(path, nodes)
      return nodes
    },
    [workspaceId],
  )

  const loadRoot = useCallback(async () => {
    if (!workspaceId) return
    setLoading(true)
    setError(null)
    try {
      const nodes = await loadDirectory('')
      setRootFiles(nodes)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed to load files')
    } finally {
      setLoading(false)
    }
  }, [workspaceId, loadDirectory])

  const loadChildren = useCallback(
    async (nodePath: string): Promise<FileNode[]> => {
      return loadDirectory(nodePath)
    },
    [loadDirectory],
  )

  const refresh = useCallback(async () => {
    cacheRef.current.clear()
    await loadRoot()
  }, [loadRoot])

  useEffect(() => {
    if (workspaceId) {
      cacheRef.current.clear()
      loadRoot()
    } else {
      setRootFiles([])
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspaceId])

  return { rootFiles, loading, error, loadChildren, refresh }
}
