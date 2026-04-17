import { describe, it, expect } from 'vitest'
import {
  getFileExtension,
  getFileIcon,
  formatFileSize,
  filterTree,
  findNodeById,
  getAllFolderIds,
} from '../../components/file-tree/utils'
import type { FileNode } from '../../components/file-tree/types'

import {
  File,
  Folder,
  FolderOpen,
  FileCode,
  FileJson,
  FileText,
  FileImage,
  FileVideo,
  FileAudio,
  FileArchive,
  FileSpreadsheet,
} from 'lucide-react'

describe('file-tree utils', () => {
  describe('getFileExtension', () => {
    it('应该返回文件扩展名', () => {
      expect(getFileExtension('file.ts')).toBe('ts')
      expect(getFileExtension('component.tsx')).toBe('tsx')
      expect(getFileExtension('style.css')).toBe('css')
    })

    it('应该返回小写的扩展名', () => {
      expect(getFileExtension('file.TS')).toBe('ts')
      expect(getFileExtension('file.TSX')).toBe('tsx')
    })

    it('没有扩展名时应该返回空字符串', () => {
      expect(getFileExtension('filename')).toBe('')
      expect(getFileExtension('.hidden')).toBe('hidden')
    })
  })

  describe('getFileIcon', () => {
    it('returns FolderOpen when folder is expanded', () => {
      const node: FileNode = { id: '1', name: 'src', type: 'folder', path: '/src' }
      expect(getFileIcon(node, true)).toBe(FolderOpen)
    })

    it('returns Folder when folder is not expanded', () => {
      const node: FileNode = { id: '1', name: 'src', type: 'folder', path: '/src' }
      expect(getFileIcon(node, false)).toBe(Folder)
    })

    it('returns Folder when folder expanded param omitted', () => {
      const node: FileNode = { id: '1', name: 'src', type: 'folder', path: '/src' }
      expect(getFileIcon(node)).toBe(Folder)
    })

    it('returns FileCode for tsx extension', () => {
      const node: FileNode = { id: '1', name: 'App.tsx', type: 'file', path: '/App.tsx' }
      expect(getFileIcon(node)).toBe(FileCode)
    })

    it('returns FileCode for py extension', () => {
      const node: FileNode = { id: '1', name: 'main.py', type: 'file', path: '/main.py' }
      expect(getFileIcon(node)).toBe(FileCode)
    })

    it('returns FileJson for json extension', () => {
      const node: FileNode = { id: '1', name: 'data.json', type: 'file', path: '/data.json' }
      expect(getFileIcon(node)).toBe(FileJson)
    })

    it('returns FileText for md extension', () => {
      const node: FileNode = { id: '1', name: 'readme.md', type: 'file', path: '/readme.md' }
      expect(getFileIcon(node)).toBe(FileText)
    })

    it('returns FileImage for png extension', () => {
      const node: FileNode = { id: '1', name: 'logo.png', type: 'file', path: '/logo.png' }
      expect(getFileIcon(node)).toBe(FileImage)
    })

    it('returns FileVideo for mp4 extension', () => {
      const node: FileNode = { id: '1', name: 'video.mp4', type: 'file', path: '/video.mp4' }
      expect(getFileIcon(node)).toBe(FileVideo)
    })

    it('returns FileAudio for mp3 extension', () => {
      const node: FileNode = { id: '1', name: 'song.mp3', type: 'file', path: '/song.mp3' }
      expect(getFileIcon(node)).toBe(FileAudio)
    })

    it('returns FileArchive for zip extension', () => {
      const node: FileNode = { id: '1', name: 'archive.zip', type: 'file', path: '/archive.zip' }
      expect(getFileIcon(node)).toBe(FileArchive)
    })

    it('returns FileSpreadsheet for xlsx extension', () => {
      const node: FileNode = { id: '1', name: 'sheet.xlsx', type: 'file', path: '/sheet.xlsx' }
      expect(getFileIcon(node)).toBe(FileSpreadsheet)
    })

    it('uses extension prop over filename', () => {
      const node: FileNode = { id: '1', name: 'data', type: 'file', path: '/data', extension: 'json' }
      expect(getFileIcon(node)).toBe(FileJson)
    })

    it('returns File for unknown extension', () => {
      const node: FileNode = { id: '1', name: 'data.xyz', type: 'file', path: '/data.xyz' }
      expect(getFileIcon(node)).toBe(File)
    })

    it('returns File for no extension', () => {
      const node: FileNode = { id: '1', name: 'Makefile', type: 'file', path: '/Makefile' }
      expect(getFileIcon(node)).toBe(File)
    })
  })

  describe('formatFileSize', () => {
    it('应该格式化字节', () => {
      expect(formatFileSize(500)).toBe('500.0 B')
    })

    it('应该格式化 KB', () => {
      expect(formatFileSize(1024)).toBe('1.0 KB')
    })

    it('应该格式化 MB', () => {
      expect(formatFileSize(1048576)).toBe('1.0 MB')
    })

    it('应该格式化 GB', () => {
      expect(formatFileSize(1073741824)).toBe('1.0 GB')
    })

    it('应该返回空字符串当bytes为0或undefined', () => {
      expect(formatFileSize(0)).toBe('')
      expect(formatFileSize(undefined as unknown)).toBe('')
    })
  })

  describe('filterTree', () => {
    it('returns all nodes when query is empty', () => {
      const nodes: FileNode[] = [{ id: '1', name: 'file.ts', type: 'file', path: '/file.ts' }]
      expect(filterTree(nodes, '')).toHaveLength(1)
    })

    it('filters files by name', () => {
      const nodes: FileNode[] = [
        { id: '1', name: 'App.tsx', type: 'file', path: '/App.tsx' },
        { id: '2', name: 'main.py', type: 'file', path: '/main.py' },
      ]
      const result = filterTree(nodes, 'app')
      expect(result).toHaveLength(1)
      expect(result[0].name).toBe('App.tsx')
    })

    it('keeps folder if child matches', () => {
      const nodes: FileNode[] = [
        {
          id: '1',
          name: 'src',
          type: 'folder',
          path: '/src',
          children: [
            { id: '1-1', name: 'App.tsx', type: 'file', path: '/src/App.tsx' },
            { id: '1-2', name: 'util.ts', type: 'file', path: '/src/util.ts' },
          ],
        },
      ]
      const result = filterTree(nodes, 'app')
      expect(result).toHaveLength(1)
      expect(result[0].children).toHaveLength(1)
    })

    it('removes folder with no matching children', () => {
      const nodes: FileNode[] = [
        {
          id: '1',
          name: 'src',
          type: 'folder',
          path: '/src',
          children: [
            { id: '1-1', name: 'App.tsx', type: 'file', path: '/src/App.tsx' },
          ],
        },
      ]
      const result = filterTree(nodes, 'readme')
      expect(result).toHaveLength(0)
    })

    it('case insensitive filter', () => {
      const nodes: FileNode[] = [
        { id: '1', name: 'App.tsx', type: 'file', path: '/App.tsx' },
      ]
      const result = filterTree(nodes, 'APP')
      expect(result).toHaveLength(1)
    })
  })

  describe('findNodeById', () => {
    it('finds top-level node', () => {
      const nodes: FileNode[] = [{ id: '1', name: 'file.ts', type: 'file', path: '/file.ts' }]
      expect(findNodeById(nodes, '1')?.name).toBe('file.ts')
    })

    it('finds nested node', () => {
      const nodes: FileNode[] = [
        {
          id: '1',
          name: 'src',
          type: 'folder',
          path: '/src',
          children: [
            { id: '1-1', name: 'App.tsx', type: 'file', path: '/src/App.tsx' },
          ],
        },
      ]
      expect(findNodeById(nodes, '1-1')?.name).toBe('App.tsx')
    })

    it('returns null when not found', () => {
      const nodes: FileNode[] = [{ id: '1', name: 'file.ts', type: 'file', path: '/file.ts' }]
      expect(findNodeById(nodes, '999')).toBeNull()
    })

    it('returns null for empty array', () => {
      expect(findNodeById([], '1')).toBeNull()
    })
  })

  describe('getAllFolderIds', () => {
    it('returns all folder IDs including nested', () => {
      const files: FileNode[] = [
        {
          id: '1',
          name: 'src',
          type: 'folder',
          path: '/src',
          children: [
            {
              id: '1-1',
              name: 'components',
              type: 'folder',
              path: '/src/components',
            },
          ],
        },
        {
          id: '2',
          name: 'public',
          type: 'folder',
          path: '/public',
        },
      ]
      const result = getAllFolderIds(files)
      expect(result).toContain('1')
      expect(result).toContain('1-1')
      expect(result).toContain('2')
      expect(result).toHaveLength(3)
    })

    it('没有文件夹时应该返回空数组', () => {
      const files: FileNode[] = [
        {
          id: '1',
          name: 'file.ts',
          type: 'file',
          path: '/file.ts',
        },
      ]
      const result = getAllFolderIds(files)
      expect(result).toHaveLength(0)
    })
  })
})
