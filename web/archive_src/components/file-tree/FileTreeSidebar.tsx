import { memo, useMemo, useState } from 'react'
import { LayoutDashboard, Search, RefreshCw, FolderTree } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { FileTreeSidebarProps } from './types'
import { FileTree } from './FileTree'
import { FileTreeProvider } from './FileTreeContext'
import { useFileTree } from './useFileTree'
import { filterTree } from './utils'

export const FileTreeSidebar = memo(function FileTreeSidebar({
  files,
  selectedId,
  onSelect,
  title = '文件',
  className,
  workspaceSlot,
}: FileTreeSidebarProps) {
  const [activeView, setActiveView] = useState<'files' | 'dashboard'>('dashboard')

  return (
    <div
      className={cn('flex flex-col h-full border-r border-border bg-background', className)}
      style={{ width: '220px' }}
    >
      {workspaceSlot}
      <button
        type="button"
        onClick={() => setActiveView(activeView === 'dashboard' ? 'files' : 'dashboard')}
        className={cn(
          'flex items-center gap-2 px-3 py-2.5 border-b border-border w-full transition-colors',
          activeView === 'dashboard'
            ? 'bg-accent/60'
            : 'hover:bg-accent/30',
        )}
      >
        <LayoutDashboard className="size-3.5" />
        <h2 className="text-xs font-semibold">仪表盘</h2>
        {activeView === 'dashboard' && (
          <span className="ml-auto text-[0.6rem] text-muted-foreground">当前</span>
        )}
      </button>
      <button
        type="button"
        onClick={() => setActiveView(activeView === 'files' ? 'dashboard' : 'files')}
        className={cn(
          'flex items-center justify-between px-3 py-2.5 border-b border-border w-full transition-colors',
          activeView === 'files'
            ? 'bg-accent/60'
            : 'hover:bg-accent/30',
        )}
      >
        <div className="flex items-center gap-2">
          <FolderTree className="size-3.5 text-muted-foreground" />
          <h2 className="text-xs font-semibold">{title}</h2>
        </div>
        <div className="flex items-center gap-1">
          {activeView !== 'files' && (
            <>
              <span className="p-1 rounded hover:bg-muted transition-colors">
                <Search className="size-3.5 text-muted-foreground" />
              </span>
              <span
                className="p-1 rounded hover:bg-muted transition-colors"
                onClick={(e) => {
                  e.stopPropagation()
                  window.location.reload()
                }}
              >
                <RefreshCw className="size-3.5 text-muted-foreground" />
              </span>
            </>
          )}
          {activeView === 'files' && (
            <span className="text-[0.6rem] text-muted-foreground">当前</span>
          )}
        </div>
      </button>

      {activeView === 'files' ? (
        <FileTreeProvider selectedId={selectedId} onSelect={onSelect}>
          <TreeContent files={files} />
        </FileTreeProvider>
      ) : (
        <SidebarDashboardHint />
      )}
    </div>
  )
})

const SidebarDashboardHint = memo(function SidebarDashboardHint() {
  return (
    <div className="flex-1 flex items-center justify-center px-4">
      <div className="text-center text-muted-foreground">
        <LayoutDashboard className="size-6 mx-auto mb-2 opacity-30" />
        <p className="text-xs">Dashboard 显示在右侧</p>
        <p className="text-[0.65rem] mt-1 text-muted-foreground/60">
          点击上方切换回文件视图
        </p>
      </div>
    </div>
  )
})

const TreeContent = memo(function TreeContent({ files }: { files: FileTreeSidebarProps['files'] }) {
  const { searchQuery } = useFileTree()

  const filteredFiles = useMemo(() => filterTree(files, searchQuery), [files, searchQuery])

  return (
    <div className="flex-1 overflow-y-auto py-2">
      {filteredFiles.length > 0 ? (
        <InnerTree files={filteredFiles} />
      ) : (
        <div className="px-4 py-8 text-center text-sm text-muted-foreground">
          没有找到匹配的文件
        </div>
      )}
    </div>
  )
})

const InnerTree = memo(function InnerTree({ files }: { files: FileTreeSidebarProps['files'] }) {
  const { selectedId, onSelect } = useFileTree()
  return <FileTree files={files} selectedId={selectedId} onSelect={onSelect} />
})
