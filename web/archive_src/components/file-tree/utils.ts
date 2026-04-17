import type { FileNode } from './types'
import {
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

export function getFileExtension(filename: string): string {
  const parts = filename.split('.')
  return parts.length > 1 ? parts[parts.length - 1].toLowerCase() : ''
}

export function getFileIcon(node: FileNode, isExpanded: boolean = false) {
  if (node.type === 'folder') {
    return isExpanded ? FolderOpen : Folder
  }

  const ext = node.extension || getFileExtension(node.name)

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

  return iconMap[ext] || File
}

export function formatFileSize(bytes?: number): string {
  if (!bytes) return ''

  const units = ['B', 'KB', 'MB', 'GB']
  let size = bytes
  let unitIndex = 0

  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024
    unitIndex++
  }

  return `${size.toFixed(1)} ${units[unitIndex]}`
}

export function filterTree(nodes: FileNode[], query: string): FileNode[] {
  if (!query) return nodes

  const lowerQuery = query.toLowerCase()

  return nodes
    .map((node) => {
      if (node.type === 'folder' && node.children) {
        const filteredChildren = filterTree(node.children, query)
        if (filteredChildren.length > 0) {
          return { ...node, children: filteredChildren }
        }
      }

      if (node.name.toLowerCase().includes(lowerQuery)) {
        return node
      }

      return null
    })
    .filter((node): node is FileNode => node !== null)
}

export function findNodeById(nodes: FileNode[], id: string): FileNode | null {
  for (const node of nodes) {
    if (node.id === id) return node

    if (node.children) {
      const found = findNodeById(node.children, id)
      if (found) return found
    }
  }

  return null
}

export function getAllFolderIds(nodes: FileNode[]): string[] {
  const ids: string[] = []

  function traverse(items: FileNode[]) {
    for (const node of items) {
      if (node.type === 'folder') {
        ids.push(node.id)
        if (node.children) {
          traverse(node.children)
        }
      }
    }
  }

  traverse(nodes)
  return ids
}
