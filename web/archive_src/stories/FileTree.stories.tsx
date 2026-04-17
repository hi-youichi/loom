import type { Meta, StoryObj } from '@storybook/react-vite'
import { FileTree, FileTreeSidebar } from '../components/file-tree'
import type { FileNode } from '../components/file-tree'

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
          {
            id: '1-1-3',
            name: 'Card.tsx',
            type: 'file',
            path: '/src/components/Card.tsx',
            size: 3072,
            extension: 'tsx',
          },
        ],
      },
      {
        id: '1-2',
        name: 'hooks',
        type: 'folder',
        path: '/src/hooks',
        children: [
          {
            id: '1-2-1',
            name: 'useAuth.ts',
            type: 'file',
            path: '/src/hooks/useAuth.ts',
            size: 4096,
            extension: 'ts',
          },
          {
            id: '1-2-2',
            name: 'useTheme.ts',
            type: 'file',
            path: '/src/hooks/useTheme.ts',
            size: 1024,
            extension: 'ts',
          },
        ],
      },
      {
        id: '1-3',
        name: 'App.tsx',
        type: 'file',
        path: '/src/App.tsx',
        size: 5120,
        extension: 'tsx',
      },
      {
        id: '1-4',
        name: 'main.tsx',
        type: 'file',
        path: '/src/main.tsx',
        size: 768,
        extension: 'tsx',
      },
    ],
  },
  {
    id: '2',
    name: 'public',
    type: 'folder',
    path: '/public',
    children: [
      {
        id: '2-1',
        name: 'images',
        type: 'folder',
        path: '/public/images',
        children: [
          {
            id: '2-1-1',
            name: 'logo.png',
            type: 'file',
            path: '/public/images/logo.png',
            size: 8192,
            extension: 'png',
          },
          {
            id: '2-1-2',
            name: 'banner.jpg',
            type: 'file',
            path: '/public/images/banner.jpg',
            size: 16384,
            extension: 'jpg',
          },
        ],
      },
      {
        id: '2-2',
        name: 'index.html',
        type: 'file',
        path: '/public/index.html',
        size: 2048,
        extension: 'html',
      },
    ],
  },
  {
    id: '3',
    name: 'package.json',
    type: 'file',
    path: '/package.json',
    size: 1536,
    extension: 'json',
  },
  {
    id: '4',
    name: 'README.md',
    type: 'file',
    path: '/README.md',
    size: 3072,
    extension: 'md',
  },
]

const meta = {
  title: 'Components/FileTree',
  component: FileTree,
  parameters: {
    layout: 'centered',
  },
  tags: ['autodocs'],
} satisfies Meta<typeof FileTree>

export default meta
type Story = StoryObj<typeof meta>

export const Default: Story = {
  args: {
    files: sampleFiles,
    onSelect: (node: FileNode) => console.log('Selected:', node),
  },
  decorators: [
    (Story) => (
      <div style={{ width: '300px', height: '500px', border: '1px solid #e5e7eb', borderRadius: '8px' }}>
        <Story />
      </div>
    ),
  ],
}

export const WithSelectedFile: Story = {
  args: {
    files: sampleFiles,
    selectedId: '1-1-1',
    onSelect: (node: FileNode) => console.log('Selected:', node),
  },
  decorators: [
    (Story) => (
      <div style={{ width: '300px', height: '500px', border: '1px solid #e5e7eb', borderRadius: '8px' }}>
        <Story />
      </div>
    ),
  ],
}

export const Sidebar: StoryObj<typeof FileTreeSidebar> = {
  render: (args) => (
    <div style={{ height: '600px', display: 'flex' }}>
      <FileTreeSidebar {...args} />
      <div style={{ flex: 1, padding: '24px', backgroundColor: '#f9fafb' }}>
        <h2 style={{ fontSize: '18px', fontWeight: '600', marginBottom: '8px' }}>内容区域</h2>
        <p style={{ color: '#6b7280' }}>选中文件的内容将显示在这里</p>
      </div>
    </div>
  ),
  args: {
    files: sampleFiles,
    selectedId: '1-1-1',
    title: '项目文件',
    onSelect: (node: FileNode) => console.log('Selected:', node),
  },
}

export const SidebarWithCustomTitle: StoryObj<typeof FileTreeSidebar> = {
  render: (args) => (
    <div style={{ height: '600px', display: 'flex' }}>
      <FileTreeSidebar {...args} />
      <div style={{ flex: 1, padding: '24px', backgroundColor: '#f9fafb' }}>
        <h2 style={{ fontSize: '18px', fontWeight: '600', marginBottom: '8px' }}>内容区域</h2>
        <p style={{ color: '#6b7280' }}>选中文件的内容将显示在这里</p>
      </div>
    </div>
  ),
  args: {
    files: sampleFiles,
    title: '文档目录',
    onSelect: (node: FileNode) => console.log('Selected:', node),
  },
}
