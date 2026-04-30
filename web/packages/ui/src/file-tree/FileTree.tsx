import { memo, useMemo } from 'react'
import { cn } from '../lib/utils'
import type { FileTreeProps } from './types'
import { FileTreeProvider } from './FileTreeContext'
import { FileTreeItem } from './FileTreeItem'
import { getAllFolderIds } from './utils'

export const FileTree = memo(function FileTree({
  files,
  selectedId,
  onSelect,
  className,
}: FileTreeProps) {
  const initialExpandedIds = useMemo(() => getAllFolderIds(files), [files])

  return (
    <FileTreeProvider
      selectedId={selectedId}
      onSelect={onSelect}
      initialExpandedIds={initialExpandedIds}
    >
      <InnerFileTree files={files} className={className} />
    </FileTreeProvider>
  )
})

const InnerFileTree = memo(function InnerFileTree({
  files,
  className,
}: {
  files: FileTreeProps['files']
  className?: string
}) {
  return (
    <div role="tree" className={cn('select-none', className)}>
      {files.map((node) => (
        <FileTreeItem key={node.id} node={node} depth={0} />
      ))}
    </div>
  )
})
