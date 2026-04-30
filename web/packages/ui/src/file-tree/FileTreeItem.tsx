import { memo, useState, useRef, useEffect } from 'react'
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
  Pencil,
  Copy,
  Trash2,
  FilePlus,
  FolderPlus,
  RefreshCw,
} from 'lucide-react'
import { cn } from '../lib/utils'
import type { FileTreeItemProps } from './types'
import { useFileTree } from './useFileTree'
import {
  ContextMenu,
  ContextMenuTrigger,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
} from '../ui/context-menu'

const iconMap: Record<string, typeof File> = {
  js: FileCode, jsx: FileCode, ts: FileCode, tsx: FileCode,
  py: FileCode, rb: FileCode, go: FileCode, rs: FileCode,
  java: FileCode, c: FileCode, cpp: FileCode, h: FileCode,
  css: FileCode, scss: FileCode, less: FileCode, html: FileCode, xml: FileCode,
  json: FileJson, md: FileText, txt: FileText, pdf: FileText,
  doc: FileText, docx: FileText,
  jpg: FileImage, jpeg: FileImage, png: FileImage, gif: FileImage,
  svg: FileImage, webp: FileImage,
  mp4: FileVideo, avi: FileVideo, mov: FileVideo, mkv: FileVideo,
  mp3: FileAudio, wav: FileAudio, flac: FileAudio, aac: FileAudio,
  zip: FileArchive, tar: FileArchive, gz: FileArchive, rar: FileArchive, '7z': FileArchive,
  xls: FileSpreadsheet, xlsx: FileSpreadsheet, csv: FileSpreadsheet,
}

function getFileExtension(filename: string): string {
  const parts = filename.split('.')
  return parts.length > 1 ? parts[parts.length - 1].toLowerCase() : ''
}

function InlineInput({ defaultValue, onCommit, onCancel }: {
  defaultValue: string
  onCommit: (value: string) => void
  onCancel: () => void
}) {
  const [value, setValue] = useState(defaultValue)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const input = inputRef.current
    if (!input) return
    input.focus()
    const dot = defaultValue.lastIndexOf('.')
    if (dot > 0) {
      input.setSelectionRange(0, dot)
    } else {
      input.select()
    }
  }, [defaultValue])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      const trimmed = value.trim()
      if (trimmed) onCommit(trimmed)
      else onCancel()
    } else if (e.key === 'Escape') {
      e.preventDefault()
      onCancel()
    }
  }

  return (
    <input
      ref={inputRef}
      data-testid="inline-rename-input"
      value={value}
      onChange={e => setValue(e.target.value)}
      onKeyDown={handleKeyDown}
      onBlur={() => {
        const trimmed = value.trim()
        if (trimmed && trimmed !== defaultValue) onCommit(trimmed)
        else onCancel()
      }}
      className="flex-1 min-w-0 px-1 py-0 text-xs bg-background border border-ring rounded-sm outline-none"
    />
  )
}

function InlineCreateInput({ type, onCommit, onCancel }: {
  type: 'file' | 'folder'
  onCommit: (name: string) => void
  onCancel: () => void
}) {
  const [value, setValue] = useState(type === 'file' ? 'untitled-1' : 'new-folder')
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const input = inputRef.current
    if (!input) return
    input.focus()
    input.select()
  }, [])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      const trimmed = value.trim()
      if (trimmed) onCommit(trimmed)
      else onCancel()
    } else if (e.key === 'Escape') {
      e.preventDefault()
      onCancel()
    }
  }

  return (
    <div
      className="flex items-center gap-1 cursor-pointer px-2 py-1"
      style={{ paddingLeft: 'inherit' }}
    >
      <span className="size-3.5 shrink-0" />
      {type === 'folder' ? (
        <Folder className="size-3.5 shrink-0 text-muted-foreground" />
      ) : (
        <File className="size-3.5 shrink-0 text-muted-foreground" />
      )}
      <input
        ref={inputRef}
        data-testid="inline-create-input"
        value={value}
        onChange={e => setValue(e.target.value)}
        onKeyDown={handleKeyDown}
        onBlur={() => {
          const trimmed = value.trim()
          if (trimmed) onCommit(trimmed)
          else onCancel()
        }}
        className="flex-1 min-w-0 px-1 py-0 text-xs bg-background border border-ring rounded-sm outline-none"
      />
    </div>
  )
}

export const FileTreeItem = memo(function FileTreeItem({ node, depth }: FileTreeItemProps) {
  const ctx = useFileTree()

  const isFolder = node.type === 'folder'
  const isExpanded = ctx.expandedIds.has(node.id)
  const isSelected = ctx.selectedId === node.id
  const isRenaming = ctx.renamingId === node.id

  const handleClick = () => {
    if (isRenaming) return
    if (isFolder) ctx.onToggle(node.id)
    ctx.onSelect(node)
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'F2') {
      e.preventDefault()
      ctx.startRename(node.id)
    } else if (e.key === 'Delete') {
      e.preventDefault()
      ctx.onDelete(node)
    }
  }

  const IconComponent = isFolder
    ? isExpanded ? FolderOpen : Folder
    : iconMap[node.extension || getFileExtension(node.name)] || File

  const indent = `${depth * 12 + 8}px`

  return (
    <div>
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div
            role="treeitem"
            aria-selected={isSelected}
            aria-expanded={isFolder ? isExpanded : undefined}
            data-testid={`file-item-${node.id}`}
            data-file-type={node.type}
            onClick={handleClick}
            onKeyDown={handleKeyDown}
            tabIndex={0}
            className={cn(
              'flex items-center gap-1 cursor-pointer px-2 py-1 rounded-md transition-colors',
              'hover:bg-muted/50',
              'focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
              isSelected && 'bg-primary/10 text-primary hover:bg-primary/15'
            )}
            style={{ paddingLeft: indent }}
          >
            {isFolder && (
              <ChevronRight
                className={cn('size-3.5 shrink-0 transition-transform', isExpanded && 'rotate-90')}
              />
            )}
            {!isFolder && <span className="size-3.5 shrink-0" />}

            <IconComponent className="size-3.5 shrink-0 text-muted-foreground" />
            {isRenaming ? (
              <InlineInput
                defaultValue={node.name}
                onCommit={(newName) => ctx.commitRename(node.id, newName)}
                onCancel={ctx.cancelRename}
              />
            ) : (
              <span className="truncate text-xs">{node.name}</span>
            )}
          </div>
        </ContextMenuTrigger>

        <ContextMenuContent data-testid={`context-menu-${node.id}`}>
          {isFolder && (
            <>
              <ContextMenuItem
                data-testid={`ctx-new-file-${node.id}`}
                onSelect={() => ctx.startCreate(node.id, 'file')}
              >
                <FilePlus className="size-3.5" />
                <span>新建文件</span>
              </ContextMenuItem>
              <ContextMenuItem
                data-testid={`ctx-new-folder-${node.id}`}
                onSelect={() => ctx.startCreate(node.id, 'folder')}
              >
                <FolderPlus className="size-3.5" />
                <span>新建文件夹</span>
              </ContextMenuItem>
              <ContextMenuSeparator />
            </>
          )}
          <ContextMenuItem
            data-testid={`ctx-rename-${node.id}`}
            onSelect={() => ctx.startRename(node.id)}
          >
            <Pencil className="size-3.5" />
            <span>重命名</span>
          </ContextMenuItem>
          <ContextMenuItem
            data-testid={`ctx-copy-path-${node.id}`}
            onSelect={() => ctx.onCopyPath(node)}
          >
            <Copy className="size-3.5" />
            <span>复制路径</span>
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem
            data-testid={`ctx-delete-${node.id}`}
            onSelect={() => ctx.onDelete(node)}
            className="text-destructive focus:text-destructive"
          >
            <Trash2 className="size-3.5" />
            <span>删除</span>
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem
            data-testid={`ctx-refresh-${node.id}`}
            onSelect={() => ctx.onRefresh()}
          >
            <RefreshCw className="size-3.5" />
            <span>刷新</span>
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>

      {isFolder && isExpanded && (
        <div role="group">
          {ctx.creatingIn === node.id && ctx.creatingType && (
            <div style={{ paddingLeft: `${(depth + 1) * 12 + 8}px` }}>
              <InlineCreateInput
                type={ctx.creatingType}
                onCommit={(name) => ctx.commitCreate(node.id, ctx.creatingType!, name)}
                onCancel={ctx.cancelCreate}
              />
            </div>
          )}
          {node.children?.map((child) => (
            <FileTreeItem key={child.id} node={child} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  )
})
