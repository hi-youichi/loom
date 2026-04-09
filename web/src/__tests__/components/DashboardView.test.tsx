import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { DashboardView } from '../../components/dashboard/DashboardView'
import type { AgentInfo, ActivityEvent } from '../../types/agent'

const agents: AgentInfo[] = [
  {
    name: 'coder',
    status: 'running',
    callCount: 5,
    lastRunAt: new Date().toISOString(),
    lastError: null,
    profile: {
      name: 'coder',
      description: '开发 Agent',
      tools: ['bash'],
      mcpServers: [],
      source: 'builtin',
    },
  },
  {
    name: 'reviewer',
    status: 'idle',
    callCount: 2,
    lastRunAt: new Date().toISOString(),
    lastError: null,
    profile: {
      name: 'reviewer',
      description: '审查 Agent',
      tools: ['read'],
      mcpServers: [],
      source: 'project',
    },
  },
]

const activity: ActivityEvent[] = [
  {
    id: 'a1',
    timestamp: new Date().toISOString(),
    agent: 'coder',
    type: 'run_start',
    summary: 'started work',
    isError: false,
  },
]

describe('DashboardView', () => {
  it('renders header title', () => {
    render(<DashboardView agents={agents} activity={activity} activeCount={1} totalCalls={7} />)
    expect(screen.getByText('Agent Dashboard')).toBeInTheDocument()
  })

  it('renders stat chips', () => {
    render(<DashboardView agents={agents} activity={activity} activeCount={1} totalCalls={7} />)
    expect(screen.getByText('活跃')).toBeInTheDocument()
    expect(screen.getByText('总计')).toBeInTheDocument()
    expect(screen.getByText('调用')).toBeInTheDocument()
  })

  it('displays active count with accent color', () => {
    render(<DashboardView agents={agents} activity={activity} activeCount={1} totalCalls={7} />)
    const activeValue = screen.getByText('1')
    expect(activeValue.className).toContain('text-emerald')
  })

  it('renders agent cards in grid', () => {
    render(<DashboardView agents={agents} activity={activity} activeCount={1} totalCalls={7} />)
    expect(screen.getByText('coder', { selector: '.text-sm.font-semibold' })).toBeInTheDocument()
    expect(screen.getByText('reviewer', { selector: '.text-sm.font-semibold' })).toBeInTheDocument()
  })

  it('renders activity feed section', () => {
    render(<DashboardView agents={agents} activity={activity} activeCount={1} totalCalls={7} />)
    expect(screen.getByText('最近活动')).toBeInTheDocument()
  })

  it('shows filter chip when agent selected', () => {
    render(<DashboardView agents={agents} activity={activity} activeCount={1} totalCalls={7} />)
    fireEvent.click(screen.getByText('coder', { selector: '.text-sm.font-semibold' }))
    expect(screen.getByText(/筛选: coder/)).toBeInTheDocument()
  })

  it('clears filter when chip dismiss clicked', () => {
    render(<DashboardView agents={agents} activity={activity} activeCount={1} totalCalls={7} />)
    fireEvent.click(screen.getByText('coder', { selector: '.text-sm.font-semibold' }))
    expect(screen.getByText(/筛选: coder/)).toBeInTheDocument()
    const dismissBtn = screen.getByText('筛选: coder').closest('button')!
    fireEvent.click(dismissBtn)
    expect(screen.queryByText(/筛选:/)).not.toBeInTheDocument()
  })

  it('calls onSelectAgent with null resets filter', () => {
    render(<DashboardView agents={agents} activity={activity} activeCount={1} totalCalls={7} />)
    fireEvent.click(screen.getByText('coder', { selector: '.text-sm.font-semibold' }))
    expect(screen.getByText(/筛选: coder/)).toBeInTheDocument()
    fireEvent.click(screen.getByText('coder', { selector: '.text-sm.font-semibold' }))
    expect(screen.queryByText(/筛选:/)).not.toBeInTheDocument()
  })

  it('shows filtered agent hint in activity section', () => {
    render(<DashboardView agents={agents} activity={activity} activeCount={1} totalCalls={7} />)
    fireEvent.click(screen.getByText('coder', { selector: '.text-sm.font-semibold' }))
    expect(screen.getByText(/仅显示 coder/)).toBeInTheDocument()
  })

  it('renders with empty agents', () => {
    render(<DashboardView agents={[]} activity={[]} activeCount={0} totalCalls={0} />)
    expect(screen.getByText('暂无 Agent 活动')).toBeInTheDocument()
    expect(screen.getByText('暂无活动记录')).toBeInTheDocument()
  })

  it('displays totalCalls stat value', () => {
    render(<DashboardView agents={agents} activity={activity} activeCount={1} totalCalls={42} />)
    expect(screen.getByText('42')).toBeInTheDocument()
  })

  it('does not show accent when activeCount is 0', () => {
    render(<DashboardView agents={agents} activity={activity} activeCount={0} totalCalls={0} />)
    const values = screen.getAllByText('0')
    const activeValue = values.find(el => el.closest('.flex-col')?.querySelector('.text-\\[0\\.6rem\\]'))
    expect(activeValue?.className).not.toContain('text-emerald')
  })
})
