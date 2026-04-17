import { memo } from 'react'
import {
  ChevronRight,
  File,
  FileText,
  FileCode,
  FileImage,
  FileVideo,
  FileAudio,
  FileArchive,
  FileSpreadsheet,
  FileJson,
  Folder,
  FolderOpen,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import type { FileTreeItemProps } from './types'
import { useFileTree } from './useFileTree'

const iconMap: Record<string, typeof File> = {
  js: FileCode,
  jsx: FileCode,
  ts: FileCode,
  tsx: FileCode,
  py: FileCode,
  rb: FileCode,
  go: FileCode,
  rs: FileCode,
  java: FileCode,
  c: FileCode,
  cpp: FileCode,
  h: FileCode,
  css: FileCode,
  scss: FileCode,
  less: FileCode,
  html: FileCode,
  xml: FileCode,
  json: FileJson,
  md: FileText,
  txt: FileText,
  pdf: FileText,
  doc: FileText,
  docx: FileText,
  jpg: FileImage,
  jpeg: FileImage,
  png: FileImage,
  gif: FileImage,
  svg: FileImage,
  webp: FileImage,
  mp4: FileVideo,
  avi: FileVideo,
  mov: FileVideo,
  mkv: FileVideo,
  mp3: FileAudio,
  wav: FileAudio,
  flac: FileAudio,
  aac: FileAudio,
  zip: FileArchive,
  tar: FileArchive,
  gz: FileArchive,
  rar: FileArchive,
  '7z': FileArchive,
  xls: FileSpreadsheet,
  xlsx: FileSpreadsheet,
  csv: FileSpreadsheet,
}

function getFileExtension(filename: string): string {
  const parts = filename.split('.')
  return parts.length > 1 ? parts[parts.length - 1].toLowerCase() : ''
}

export const FileTreeItem = memo(function FileTreeItem({ node, depth }: FileTreeItemProps) {
  const { selectedId, expandedIds, onSelect, onToggle } = useFileTree()

  const isFolder = node.type === 'folder'
  const isExpanded = expandedIds.has(node.id)
  const isSelected = selectedId === node.id

  const handleClick = () => {
    if (isFolder) {
      onToggle(node.id)
    }
    onSelect(node)
  }

  const IconComponent = isFolder
    ? isExpanded
      ? FolderOpen
      : Folder
    : iconMap[node.extension || getFileExtension(node.name)] || File

  return (
    <div>
      <div
        role="treeitem"
        aria-selected={isSelected}
        aria-expanded={isFolder ? isExpanded : undefined}
        onClick={handleClick}
        className={cn(
          'flex items-center gap-1 cursor-pointer px-2 py-1 rounded-md transition-colors',
          'hover:bg-muted/50',
          isSelected && 'bg-primary/10 text-primary hover:bg-primary/15'
        )}
        style={{ paddingLeft: `${depth * 12 + 8}px` }}
      >
        {isFolder && (
          <ChevronRight
            className={cn('size-3.5 shrink-0 transition-transform', isExpanded && 'rotate-90')}
          />
        )}
        {!isFolder && <span className="size-3.5 shrink-0" />}

        <IconComponent className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="truncate text-xs">{node.name}</span>
      </div>

      {isFolder && isExpanded && node.children && (
        <div role="group">
          {node.children.map((child) => (
            <FileTreeItem key={child.id} node={child} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  )
})
