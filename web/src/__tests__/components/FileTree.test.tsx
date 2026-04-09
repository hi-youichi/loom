import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { FileTree } from '../../components/file-tree/FileTree'
import type { FileNode } from '../../components/file-tree/types'

describe('FileTree', () => {
  const sampleFiles: FileNode[] = [
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
    {
      id: '2',
      name: 'package.json',
      type: 'file',
      path: '/package.json',
      extension: 'json',
    },
  ]

  it('应该渲染文件和文件夹', () => {
    render(<FileTree files={sampleFiles} />)

    expect(screen.getByText('src')).toBeInTheDocument()
    expect(screen.getByText('package.json')).toBeInTheDocument()
  })

  it('默认展开所有文件夹', () => {
    render(<FileTree files={sampleFiles} />)

    expect(screen.getByText('App.tsx')).toBeInTheDocument()
  })

  it('应该应用自定义 className', () => {
    const { container } = render(<FileTree files={sampleFiles} className="custom-class" />)

    expect(container.firstChild).toHaveClass('custom-class')
  })

  it('应该显示正确的 ARIA 属性', () => {
    render(<FileTree files={sampleFiles} />)

    const tree = screen.getByRole('tree')
    expect(tree).toBeInTheDocument()

    const treeitems = screen.getAllByRole('treeitem')
    expect(treeitems.length).toBeGreaterThan(0)
  })

  it('点击文件夹应该切换折叠状态', () => {
    render(<FileTree files={sampleFiles} />)

    expect(screen.getByText('App.tsx')).toBeInTheDocument()

    fireEvent.click(screen.getByText('src'))
    expect(screen.queryByText('App.tsx')).not.toBeInTheDocument()

    fireEvent.click(screen.getByText('src'))
    expect(screen.getByText('App.tsx')).toBeInTheDocument()
  })

  it('点击文件应该调用 onSelect', () => {
    const handleSelect = vi.fn()
    render(<FileTree files={sampleFiles} onSelect={handleSelect} />)

    fireEvent.click(screen.getByText('package.json'))

    expect(handleSelect).toHaveBeenCalledWith(
      expect.objectContaining({
        id: '2',
        name: 'package.json',
        type: 'file',
      })
    )
  })

  it('应该高亮选中的文件', () => {
    render(<FileTree files={sampleFiles} selectedId="2" />)

    const selectedFile = screen.getByText('package.json')
    expect(selectedFile.closest('[aria-selected="true"]')).toBeInTheDocument()
  })

  it('空数组时不崩溃', () => {
    const { container } = render(<FileTree files={[]} />)
    expect(container.querySelector('[role="tree"]')).toBeInTheDocument()
  })

  it('renders file without extension using default icon', () => {
    const files: FileNode[] = [
      { id: '3', name: 'Makefile', type: 'file', path: '/Makefile' },
    ]
    render(<FileTree files={files} />)
    expect(screen.getByText('Makefile')).toBeInTheDocument()
  })

  it('renders file with extension in filename but no extension prop', () => {
    const files: FileNode[] = [
      { id: '4', name: 'App.tsx', type: 'file', path: '/App.tsx' },
    ]
    render(<FileTree files={files} />)
    expect(screen.getByText('App.tsx')).toBeInTheDocument()
  })
})
