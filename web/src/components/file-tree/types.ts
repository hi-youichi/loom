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
}
