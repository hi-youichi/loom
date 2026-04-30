export interface FileNode {
  id: string
  name: string
  type: 'file' | 'folder'
  children?: FileNode[]
  path: string
  size?: number
  modifiedAt?: Date
  extension?: string
}

export interface FileTreeContextValue {
  selectedId: string | null
  expandedIds: Set<string>
  searchQuery: string
  onSelect: (node: FileNode) => void
  onToggle: (id: string) => void
  setSearchQuery: (query: string) => void
  renamingId: string | null
  startRename: (id: string) => void
  cancelRename: () => void
  commitRename: (id: string, newName: string) => void
  creatingIn: string | null
  creatingType: 'file' | 'folder' | null
  startCreate: (parentId: string, type: 'file' | 'folder') => void
  cancelCreate: () => void
  commitCreate: (parentId: string, type: 'file' | 'folder', name: string) => void
  onDelete: (node: FileNode) => void
  onCopyPath: (node: FileNode) => void
  onRefresh: () => void
}

export interface FileTreeProps {
  files: FileNode[]
  selectedId?: string | null
  onSelect?: (node: FileNode) => void
  className?: string
}

export interface FileTreeItemProps {
  node: FileNode
  depth: number
}

export interface FileTreeSidebarProps {
  files: FileNode[]
  selectedId?: string | null
  onSelect?: (node: FileNode) => void
  title?: string
  className?: string
  workspaceSlot?: React.ReactNode
  loading?: boolean
  onRename?: (node: FileNode, newName: string) => void
  onDelete?: (node: FileNode) => void
  onCreateFile?: (parentId: string, name: string) => void
  onCreateFolder?: (parentId: string, name: string) => void
  onRefresh?: () => void
}
