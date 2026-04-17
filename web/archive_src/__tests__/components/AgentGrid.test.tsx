import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AgentGrid } from '../../components/dashboard/AgentGrid'
import type { AgentInfo } from '../../types/agent'

function makeAgent(overrides: Partial<AgentInfo> = {}): AgentInfo {
  return {
    name: 'dev',
    status: 'idle',
    callCount: 1,
    lastRunAt: new Date().toISOString(),
    lastError: null,
    ...overrides,
  }
}

describe('AgentGrid', () => {
  it('shows empty state when no agents', () => {
    render(<AgentGrid agents={[]} selectedAgent={null} onSelectAgent={vi.fn()} />)
    expect(screen.getByText('暂无 Agent 活动')).toBeInTheDocument()
    expect(screen.getByText('发送消息后，Agent 会自动出现在这里')).toBeInTheDocument()
  })

  it('renders all agent cards', () => {
    const agents = [
      makeAgent({ name: 'dev' }),
      makeAgent({ name: 'reviewer' }),
    ]
    render(<AgentGrid agents={agents} selectedAgent={null} onSelectAgent={vi.fn()} />)
    expect(screen.getByText('dev')).toBeInTheDocument()
    expect(screen.getByText('reviewer')).toBeInTheDocument()
  })

  it('sorts agents by status priority: running > error > idle', () => {
    const agents = [
      makeAgent({ name: 'idle-agent', status: 'idle' }),
      makeAgent({ name: 'running-agent', status: 'running' }),
      makeAgent({ name: 'error-agent', status: 'error' }),
    ]
    render(<AgentGrid agents={agents} selectedAgent={null} onSelectAgent={vi.fn()} />)
    const buttons = screen.getAllByRole('button')
    expect(buttons[0]).toHaveTextContent('running-agent')
    expect(buttons[1]).toHaveTextContent('error-agent')
    expect(buttons[2]).toHaveTextContent('idle-agent')
  })

  it('sorts agents with same status by lastRunAt desc', () => {
    const now = Date.now()
    const agents = [
      makeAgent({ name: 'older', status: 'idle', lastRunAt: new Date(now - 10000).toISOString() }),
      makeAgent({ name: 'newer', status: 'idle', lastRunAt: new Date(now).toISOString() }),
    ]
    render(<AgentGrid agents={agents} selectedAgent={null} onSelectAgent={vi.fn()} />)
    const buttons = screen.getAllByRole('button')
    expect(buttons[0]).toHaveTextContent('newer')
    expect(buttons[1]).toHaveTextContent('older')
  })

  it('highlights selected agent card', () => {
    const agents = [makeAgent({ name: 'dev' }), makeAgent({ name: 'reviewer' })]
    render(<AgentGrid agents={agents} selectedAgent="dev" onSelectAgent={vi.fn()} />)
    const buttons = screen.getAllByRole('button')
    expect(buttons[0].className).toContain('border-foreground')
    expect(buttons[1].className).not.toContain('border-foreground')
  })

  it('calls onSelectAgent when card clicked', () => {
    const onSelect = vi.fn()
    const agents = [makeAgent({ name: 'dev' })]
    render(<AgentGrid agents={agents} selectedAgent={null} onSelectAgent={onSelect} />)
    fireEvent.click(screen.getByRole('button'))
    expect(onSelect).toHaveBeenCalledWith('dev')
  })

  it('handles agents with null lastRunAt in sorting', () => {
    const agents = [
      makeAgent({ name: 'no-date', status: 'idle', lastRunAt: null }),
      makeAgent({ name: 'has-date', status: 'idle', lastRunAt: new Date().toISOString() }),
    ]
    render(<AgentGrid agents={agents} selectedAgent={null} onSelectAgent={vi.fn()} />)
    expect(screen.getByText('no-date')).toBeInTheDocument()
    expect(screen.getByText('has-date')).toBeInTheDocument()
  })
})
