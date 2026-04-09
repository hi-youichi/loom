import { useState } from 'react'
import { FileTreeSidebar, type FileNode } from '@/components/file-tree'

const sampleFiles: FileNode[] = [
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
        children: [
          {
            id: '1-1-1',
            name: 'Button.tsx',
            type: 'file',
            path: '/src/components/Button.tsx',
            size: 2048,
            extension: 'tsx',
          },
          {
            id: '1-1-2',
            name: 'Input.tsx',
            type: 'file',
            path: '/src/components/Input.tsx',
            size: 1536,
            extension: 'tsx',
          },
        ],
      },
      {
        id: '1-2',
        name: 'App.tsx',
        type: 'file',
        path: '/src/App.tsx',
        size: 5120,
        extension: 'tsx',
      },
    ],
  },
  {
    id: '2',
    name: 'package.json',
    type: 'file',
    path: '/package.json',
    size: 1536,
    extension: 'json',
  },
]

export default function FileTreeExample() {
  const [selectedFile, setSelectedFile] = useState<FileNode | null>(null)

  return (
    <div className="flex h-screen">
      <FileTreeSidebar
        files={sampleFiles}
        selectedId={selectedFile?.id}
        onSelect={setSelectedFile}
        title="项目文件"
      />
      <main className="flex-1 p-6 bg-muted/30">
        {selectedFile ? (
          <div>
            <h2 className="text-lg font-semibold mb-2">{selectedFile.name}</h2>
            <p className="text-sm text-muted-foreground">路径: {selectedFile.path}</p>
            {selectedFile.size && (
              <p className="text-sm text-muted-foreground">大小: {(selectedFile.size / 1024).toFixed(2)} KB</p>
            )}
          </div>
        ) : (
          <p className="text-muted-foreground">请从左侧选择一个文件</p>
        )}
      </main>
    </div>
  )
}
