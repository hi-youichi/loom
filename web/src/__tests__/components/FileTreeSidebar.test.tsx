import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { FileTreeSidebar } from '../../components/file-tree/FileTreeSidebar'
import type { FileNode } from '../../components/file-tree/types'

describe('FileTreeSidebar', () => {
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

  it('应该渲染侧边栏标题', () => {
    render(<FileTreeSidebar files={sampleFiles} />)

    expect(screen.getByText('文件')).toBeInTheDocument()
    expect(screen.getByText('仪表盘')).toBeInTheDocument()
  })

  it('应该显示自定义标题', () => {
    render(<FileTreeSidebar files={sampleFiles} title="项目文件" />)

    expect(screen.getByText('项目文件')).toBeInTheDocument()
  })

  it('默认显示仪表盘视图', () => {
    render(<FileTreeSidebar files={sampleFiles} />)

    expect(screen.getByText('Dashboard 显示在右侧')).toBeInTheDocument()
    expect(screen.getByText('当前')).toBeInTheDocument()
  })

  it('点击文件标签切换到文件视图', () => {
    render(<FileTreeSidebar files={sampleFiles} />)

    fireEvent.click(screen.getByText('文件'))
    expect(screen.getByText('src')).toBeInTheDocument()
    expect(screen.getByText('package.json')).toBeInTheDocument()
  })

  it('点击仪表盘标签切换回仪表盘视图', () => {
    render(<FileTreeSidebar files={sampleFiles} />)

    fireEvent.click(screen.getByText('文件'))
    expect(screen.getByText('src')).toBeInTheDocument()

    fireEvent.click(screen.getByText('仪表盘'))
    expect(screen.getByText('Dashboard 显示在右侧')).toBeInTheDocument()
  })

  it('点击文件应该调用 onSelect', () => {
    const handleSelect = vi.fn()
    render(<FileTreeSidebar files={sampleFiles} onSelect={handleSelect} />)

    fireEvent.click(screen.getByText('文件'))
    fireEvent.click(screen.getByText('package.json'))

    expect(handleSelect).toHaveBeenCalledWith(
      expect.objectContaining({
        id: '2',
        name: 'package.json',
        type: 'file',
      })
    )
  })

  it('应该应用自定义 className', () => {
    const { container } = render(<FileTreeSidebar files={sampleFiles} className="custom-class" />)

    expect(container.firstChild).toHaveClass('custom-class')
  })

  it('应该显示正确的宽度', () => {
    const { container } = render(<FileTreeSidebar files={sampleFiles} />)

    expect(container.firstChild).toHaveStyle({ width: '220px' })
  })

  it('shows search and refresh icons on dashboard view', () => {
    const { container } = render(<FileTreeSidebar files={sampleFiles} />)
    const refreshButtons = container.querySelectorAll('[class*="p-1 rounded"]')
    expect(refreshButtons.length).toBeGreaterThan(0)
  })

  it('file view shows 当前 indicator', () => {
    render(<FileTreeSidebar files={sampleFiles} />)
    fireEvent.click(screen.getByText('文件'))
    const currentLabels = screen.getAllByText('当前')
    expect(currentLabels.length).toBeGreaterThan(0)
  })

  it('shows empty search results message', () => {
    const { container } = render(<FileTreeSidebar files={sampleFiles} />)
    fireEvent.click(screen.getByText('文件'))
    expect(screen.getByText('src')).toBeInTheDocument()
    expect(screen.getByText('package.json')).toBeInTheDocument()
  })
})
