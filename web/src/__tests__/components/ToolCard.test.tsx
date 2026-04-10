import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ToolCard } from '../../components/ToolCard'
import type { ToolBlock } from '../../types/chat'

function createToolBlock(overrides: Partial<ToolBlock> = {}): ToolBlock {
  return {
    id: 'tool-1',
    type: 'tool',
    callId: 'call-1',
    name: 'read',
    status: 'done',
    argumentsText: '{"path": "src/main.ts"}',
    outputText: 'file contents here',
    resultText: 'success',
    isError: false,
    ...overrides,
  }
}

describe('ToolCard', () => {
  it('renders tool name in header', () => {
    render(<ToolCard tool={createToolBlock({ name: 'read' })} />)
    expect(screen.getByRole('article')).toHaveAttribute('aria-label', expect.stringContaining('Read'))
  })

  it('renders with path title', () => {
    render(<ToolCard tool={createToolBlock({ name: 'read', argumentsText: '{"path": "src/main.ts"}' })} />)
    expect(screen.getByRole('article')).toHaveAttribute('aria-label', expect.stringContaining('src/main.ts'))
  })

  it('renders without path when args have no path', () => {
    render(<ToolCard tool={createToolBlock({ name: 'read', argumentsText: '{}' })} />)
    expect(screen.getByRole('article')).toHaveAttribute('aria-label', 'Read ')
  })

  it('expands on header click', () => {
    render(<ToolCard tool={createToolBlock()} />)
    const header = screen.getByRole('button')
    fireEvent.click(header)
    expect(screen.getByRole('region')).toBeInTheDocument()
  })

  it('collapses on second click', () => {
    render(<ToolCard tool={createToolBlock()} />)
    const header = screen.getByRole('button')
    fireEvent.click(header)
    expect(screen.getByRole('region')).toBeInTheDocument()
    fireEvent.click(header)
    expect(screen.queryByRole('region')).not.toBeInTheDocument()
  })

  it('shows input params when expanded', () => {
    render(<ToolCard tool={createToolBlock({ argumentsText: '{"path": "test.ts"}' })} />)
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('输入参数')).toBeInTheDocument()
  })

  it('hides input params when no arguments', () => {
    render(<ToolCard tool={createToolBlock({ argumentsText: '' })} />)
    fireEvent.click(screen.getByRole('button'))
    expect(screen.queryByText('输入参数')).not.toBeInTheDocument()
  })

  it('shows output when expanded', () => {
    render(<ToolCard tool={createToolBlock({ outputText: 'file contents' })} />)
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('执行输出')).toBeInTheDocument()
  })

  it('hides output when no outputText', () => {
    render(<ToolCard tool={createToolBlock({ outputText: '' })} />)
    fireEvent.click(screen.getByRole('button'))
    expect(screen.queryByText('执行输出')).not.toBeInTheDocument()
  })

  it('shows result when expanded', () => {
    render(<ToolCard tool={createToolBlock({ resultText: 'success' })} />)
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('执行结果')).toBeInTheDocument()
  })

  it('hides result when no resultText', () => {
    render(<ToolCard tool={createToolBlock({ resultText: '' })} />)
    fireEvent.click(screen.getByRole('button'))
    expect(screen.queryByText('执行结果')).not.toBeInTheDocument()
  })

  it('shows expand more button for long output', () => {
    const longOutput = Array.from({ length: 10 }, (_, i) => `line ${i}`).join('\n')
    render(<ToolCard tool={createToolBlock({ outputText: longOutput })} />)
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText(/展开更多/)).toBeInTheDocument()
  })

  it('toggles full output display', () => {
    const longOutput = Array.from({ length: 10 }, (_, i) => `line ${i}`).join('\n')
    render(<ToolCard tool={createToolBlock({ outputText: longOutput })} />)
    fireEvent.click(screen.getByRole('button'))
    
    const expandBtn = screen.getByText(/展开更多/)
    fireEvent.click(expandBtn)
    expect(screen.getByText('收起内容')).toBeInTheDocument()
    
    fireEvent.click(screen.getByText('收起内容'))
    expect(screen.getByText(/展开更多/)).toBeInTheDocument()
  })

  it('starts expanded when defaultExpanded is true', () => {
    render(<ToolCard tool={createToolBlock()} defaultExpanded={true} />)
    expect(screen.getByRole('region')).toBeInTheDocument()
  })

  it('shows retry button on error status with onAction', () => {
    render(
      <ToolCard
        tool={createToolBlock({ status: 'error' })}
        onAction={vi.fn()}
      />
    )
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('重试')).toBeInTheDocument()
  })

  it('shows approve button on approval_required status with onAction', () => {
    render(
      <ToolCard
        tool={createToolBlock({ status: 'approval_required' })}
        onAction={vi.fn()}
      />
    )
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('批准')).toBeInTheDocument()
  })

  it('calls onAction with retry when retry button clicked', () => {
    const onAction = vi.fn()
    render(
      <ToolCard
        tool={createToolBlock({ status: 'error' })}
        onAction={onAction}
      />
    )
    fireEvent.click(screen.getByRole('button'))
    fireEvent.click(screen.getByText('重试'))
    expect(onAction).toHaveBeenCalledWith('retry', expect.any(Object))
  })

  it('calls onAction with approve when approve button clicked', () => {
    const onAction = vi.fn()
    render(
      <ToolCard
        tool={createToolBlock({ status: 'approval_required' })}
        onAction={onAction}
      />
    )
    fireEvent.click(screen.getByRole('button'))
    fireEvent.click(screen.getByText('批准'))
    expect(onAction).toHaveBeenCalledWith('approve', expect.any(Object))
  })

  it('does not show action buttons without onAction', () => {
    render(<ToolCard tool={createToolBlock({ status: 'error' })} />)
    fireEvent.click(screen.getByRole('button'))
    expect(screen.queryByText('重试')).not.toBeInTheDocument()
  })

  it('detects edit tool type from name', () => {
    render(<ToolCard tool={createToolBlock({ name: 'edit_file' })} />)
    expect(screen.getByRole('article').className).toContain('tool-card--edit')
  })

  it('detects delete tool type from name', () => {
    render(<ToolCard tool={createToolBlock({ name: 'delete_item' })} />)
    expect(screen.getByRole('article').className).toContain('tool-card--delete')
  })

  it('detects move tool type from name', () => {
    render(<ToolCard tool={createToolBlock({ name: 'move_file' })} />)
    expect(screen.getByRole('article').className).toContain('tool-card--move')
  })

  it('detects search tool type from name', () => {
    render(<ToolCard tool={createToolBlock({ name: 'grep_search' })} />)
    expect(screen.getByRole('article').className).toContain('tool-card--search')
  })

  it('detects execute tool type from name', () => {
    render(<ToolCard tool={createToolBlock({ name: 'execute_command' })} />)
    expect(screen.getByRole('article').className).toContain('tool-card--execute')
  })

  it('detects think tool type from name', () => {
    render(<ToolCard tool={createToolBlock({ name: 'think_tool' })} />)
    expect(screen.getByRole('article').className).toContain('tool-card--think')
  })

  it('detects fetch tool type from name', () => {
    render(<ToolCard tool={createToolBlock({ name: 'http_request' })} />)
    expect(screen.getByRole('article').className).toContain('tool-card--fetch')
  })

  it('defaults to other tool type for unknown names', () => {
    render(<ToolCard tool={createToolBlock({ name: 'custom_tool' })} />)
    expect(screen.getByRole('article').className).toContain('tool-card--other')
  })

  it('uses toolType prop when provided', () => {
    render(<ToolCard tool={createToolBlock({ toolType: 'execute', name: 'custom' })} />)
    expect(screen.getByRole('article').className).toContain('tool-card--execute')
  })

  it('handles invalid JSON in argumentsText', () => {
    render(<ToolCard tool={createToolBlock({ argumentsText: 'not json' })} />)
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('输入参数')).toBeInTheDocument()
  })

  it('expands on Enter key', () => {
    render(<ToolCard tool={createToolBlock()} />)
    const header = screen.getByRole('button')
    fireEvent.keyDown(header, { key: 'Enter' })
    expect(screen.getByRole('region')).toBeInTheDocument()
  })

  it('expands on Space key', () => {
    render(<ToolCard tool={createToolBlock()} />)
    const header = screen.getByRole('button')
    fireEvent.keyDown(header, { key: ' ' })
    expect(screen.getByRole('region')).toBeInTheDocument()
  })

  it('shows running status class', () => {
    render(<ToolCard tool={createToolBlock({ status: 'running' })} />)
    expect(screen.getByRole('article').className).toContain('tool-card--running')
  })

  it('shows queued status class', () => {
    render(<ToolCard tool={createToolBlock({ status: 'queued' })} />)
    expect(screen.getByRole('article').className).toContain('tool-card--queued')
  })
})
