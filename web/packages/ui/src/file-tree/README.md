# FileTree 组件

树形文件列表组件，支持展开/折叠、搜索过滤、文件图标等功能。

## 组件结构

- `FileTree` - 核心树形列表组件
- `FileTreeSidebar` - 带搜索框和标题的侧边栏容器
- `FileTreeItem` - 单个文件/文件夹项（内部组件）

## 使用示例

### 基础用法

\`\`\`tsx
import { FileTree, type FileNode } from '@/components/file-tree'

const files: FileNode[] = [
  {
    id: '1',
    name: 'src',
    type: 'folder',
    path: '/src',
    children: [
      {
        id: '1-1',
        name: 'App.tsx',
        type: 'file',
        path: '/src/App.tsx',
        extension: 'tsx',
      },
    ],
  },
]

function MyComponent() {
  const handleSelect = (node: FileNode) => {
    console.log('Selected:', node)
  }

  return <FileTree files={files} onSelect={handleSelect} />
}
\`\`\`

### 侧边栏用法

\`\`\`tsx
import { FileTreeSidebar, type FileNode } from '@/components/file-tree'

function Layout() {
  const [selectedFile, setSelectedFile] = useState<FileNode | null>(null)

  return (
    <div className="flex h-screen">
      <FileTreeSidebar
        files={files}
        selectedId={selectedFile?.id}
        onSelect={setSelectedFile}
        title="项目文件"
      />
      <main className="flex-1">
        {/* 内容区域 */}
      </main>
    </div>
  )
}
\`\`\`

## 数据结构

\`\`\`typescript
interface FileNode {
  id: string              // 唯一标识
  name: string            // 文件名
  type: 'file' | 'folder' // 类型
  children?: FileNode[]   // 子节点（文件夹专用）
  path: string            // 文件路径
  size?: number           // 文件大小（字节）
  modifiedAt?: Date       // 修改时间
  extension?: string      // 文件扩展名
}
\`\`\`

## 功能特性

- ✅ 树形结构展示
- ✅ 展开/折叠文件夹
- ✅ 文件类型图标（支持 30+ 种文件类型）
- ✅ 搜索过滤
- ✅ 键盘导航支持
- ✅ 选中状态管理
- ✅ 深度缩进
- ✅ Tailwind CSS 样式

## 支持的文件图标

代码文件: js, jsx, ts, tsx, py, rb, go, rs, java, c, cpp, css, scss, html, json

文档文件: md, txt, pdf, doc, docx

图片文件: jpg, jpeg, png, gif, svg, webp

音视频: mp4, avi, mov, mp3, wav, flac

压缩文件: zip, tar, gz, rar, 7z

表格文件: xls, xlsx, csv

## Storybook

运行 `npm run storybook` 查看交互式示例。
