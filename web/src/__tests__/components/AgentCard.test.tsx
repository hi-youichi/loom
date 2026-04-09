import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AgentCard } from '../../components/dashboard/AgentCard'
import type { AgentInfo } from '../../types/agent'

const baseAgent: AgentInfo = {
  name: 'dev',
  status: 'idle',
  callCount: 5,
  lastRunAt: new Date().toISOString(),
  lastError: null,
  profile: {
    name: 'dev',
    description: '开发 Agent',
    tools: ['bash', 'read'],
    mcpServers: [],
    source: 'builtin',
  },
}

describe('AgentCard', () => {
  it('renders agent name', () => {
    render(<AgentCard agent={baseAgent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText('dev')).toBeInTheDocument()
  })

  it('renders call count', () => {
    render(<AgentCard agent={baseAgent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText('5')).toBeInTheDocument()
  })

  it('calls onSelect with agent name on click', () => {
    const onSelect = vi.fn()
    render(<AgentCard agent={baseAgent} selected={false} onSelect={onSelect} />)
    fireEvent.click(screen.getByRole('button'))
    expect(onSelect).toHaveBeenCalledWith('dev')
  })

  it('shows selected state', () => {
    const { container } = render(
      <AgentCard agent={baseAgent} selected={true} onSelect={vi.fn()} />
    )
    const btn = container.querySelector('button')
    expect(btn?.className).toContain('border-foreground')
  })

  it('shows running status with pulse animation', () => {
    const agent = { ...baseAgent, status: 'running' as const }
    const { container } = render(
      <AgentCard agent={agent} selected={false} onSelect={vi.fn()} />
    )
    const dot = container.querySelector('.animate-pulse')
    expect(dot).toBeInTheDocument()
  })

  it('shows error message when status is error and lastError is set', () => {
    const agent = {
      ...baseAgent,
      status: 'error' as const,
      lastError: 'Something went wrong',
    }
    render(<AgentCard agent={agent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText('Something went wrong')).toBeInTheDocument()
  })

  it('does not show error block when status is error but no lastError', () => {
    const agent = { ...baseAgent, status: 'error' as const, lastError: null }
    render(<AgentCard agent={agent} selected={false} onSelect={vi.fn()} />)
    expect(screen.queryByText('Something went wrong')).not.toBeInTheDocument()
  })

  it('shows tool badges for visible tools', () => {
    render(<AgentCard agent={baseAgent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText('bash')).toBeInTheDocument()
    expect(screen.getByText('read')).toBeInTheDocument()
  })

  it('shows overflow count when tools exceed max visible', () => {
    const agent = {
      ...baseAgent,
      profile: {
        ...baseAgent.profile!,
        tools: ['bash', 'read', 'edit', 'write_file'],
      },
    }
    render(<AgentCard agent={agent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText('+1')).toBeInTheDocument()
  })

  it('shows source label', () => {
    render(<AgentCard agent={baseAgent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText('builtin')).toBeInTheDocument()
  })

  it('shows project source', () => {
    const agent = {
      ...baseAgent,
      profile: { ...baseAgent.profile!, source: 'project' as const },
    }
    render(<AgentCard agent={agent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText('project')).toBeInTheDocument()
  })

  it('shows user source', () => {
    const agent = {
      ...baseAgent,
      profile: { ...baseAgent.profile!, source: 'user' as const },
    }
    render(<AgentCard agent={agent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText('user')).toBeInTheDocument()
  })

  it('defaults to builtin source when no profile', () => {
    const agent = { ...baseAgent, profile: undefined }
    render(<AgentCard agent={agent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText('builtin')).toBeInTheDocument()
  })

  it('shows dash when no lastRunAt', () => {
    const agent = { ...baseAgent, lastRunAt: null }
    render(<AgentCard agent={agent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText('-')).toBeInTheDocument()
  })

  it('shows relative time for recent lastRunAt', () => {
    const agent = { ...baseAgent, lastRunAt: new Date().toISOString() }
    render(<AgentCard agent={agent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText('刚刚')).toBeInTheDocument()
  })

  it('renders idle status dot without pulse', () => {
    const { container } = render(
      <AgentCard agent={baseAgent} selected={false} onSelect={vi.fn()} />
    )
    const dot = container.querySelector('.animate-pulse')
    expect(dot).not.toBeInTheDocument()
  })

  it('does not render tool section when no tools', () => {
    const agent = {
      ...baseAgent,
      profile: { ...baseAgent.profile!, tools: [] },
    }
    render(<AgentCard agent={agent} selected={false} onSelect={vi.fn()} />)
    expect(screen.queryByText('bash')).not.toBeInTheDocument()
  })

  it('shows seconds ago for recent timestamp', () => {
    const tenSecondsAgo = new Date(Date.now() - 10000).toISOString()
    const agent = { ...baseAgent, lastRunAt: tenSecondsAgo }
    render(<AgentCard agent={agent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText(/10s前/)).toBeInTheDocument()
  })

  it('shows minutes ago for older timestamp', () => {
    const fiveMinAgo = new Date(Date.now() - 5 * 60_000).toISOString()
    const agent = { ...baseAgent, lastRunAt: fiveMinAgo }
    render(<AgentCard agent={agent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText(/5m前/)).toBeInTheDocument()
  })

  it('shows hours ago for much older timestamp', () => {
    const twoHoursAgo = new Date(Date.now() - 2 * 3_600_000).toISOString()
    const agent = { ...baseAgent, lastRunAt: twoHoursAgo }
    render(<AgentCard agent={agent} selected={false} onSelect={vi.fn()} />)
    expect(screen.getByText(/2h前/)).toBeInTheDocument()
  })
})
