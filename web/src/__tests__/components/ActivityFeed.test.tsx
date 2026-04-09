import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ActivityFeed } from '../../components/dashboard/ActivityFeed'
import type { ActivityEvent } from '../../types/agent'

function makeEvent(overrides: Partial<ActivityEvent> = {}): ActivityEvent {
  return {
    id: 'e1',
    timestamp: new Date().toISOString(),
    agent: 'dev',
    type: 'run_start',
    summary: 'started',
    isError: false,
    ...overrides,
  }
}

describe('ActivityFeed', () => {
  it('shows empty state when no events', () => {
    render(<ActivityFeed events={[]} filterAgent={null} />)
    expect(screen.getByText('暂无活动记录')).toBeInTheDocument()
  })

  it('shows empty state hint', () => {
    render(<ActivityFeed events={[]} filterAgent={null} />)
    expect(screen.getByText('Agent 开始运行后会显示事件流')).toBeInTheDocument()
  })

  it('renders events', () => {
    const events = [makeEvent({ id: 'e1', summary: 'did something' })]
    render(<ActivityFeed events={events} filterAgent={null} />)
    expect(screen.getByText('did something')).toBeInTheDocument()
  })

  it('filters events by agent', () => {
    const events = [
      makeEvent({ id: 'e1', agent: 'dev', summary: 'dev task' }),
      makeEvent({ id: 'e2', agent: 'reviewer', summary: 'review task' }),
    ]
    render(<ActivityFeed events={events} filterAgent="dev" />)
    expect(screen.getByText('dev task')).toBeInTheDocument()
    expect(screen.queryByText('review task')).not.toBeInTheDocument()
  })

  it('shows filtered empty state when agent has no events', () => {
    const events = [makeEvent({ agent: 'dev' })]
    render(<ActivityFeed events={events} filterAgent="reviewer" />)
    expect(screen.getByText('该 Agent 暂无活动记录')).toBeInTheDocument()
  })

  it('shows error styling for error events', () => {
    const events = [makeEvent({ isError: true, summary: 'failed' })]
    const { container } = render(<ActivityFeed events={events} filterAgent={null} />)
    const errorEl = container.querySelector('.bg-red-50\\/60')
    expect(errorEl).toBeInTheDocument()
  })

  it('renders agent name', () => {
    const events = [makeEvent({ agent: 'dev' })]
    render(<ActivityFeed events={events} filterAgent={null} />)
    expect(screen.getByText('dev')).toBeInTheDocument()
  })

  it('renders event without summary', () => {
    const events = [makeEvent({ summary: null })]
    render(<ActivityFeed events={events} filterAgent={null} />)
    expect(screen.getByText('dev')).toBeInTheDocument()
  })

  it('displays error label for error events', () => {
    const events = [makeEvent({ isError: true, summary: 'crash' })]
    render(<ActivityFeed events={events} filterAgent={null} />)
    expect(screen.getByText('error')).toBeInTheDocument()
  })

  it('renders tool_call event type', () => {
    const events = [makeEvent({ type: 'tool_call', summary: 'read' })]
    render(<ActivityFeed events={events} filterAgent={null} />)
    expect(screen.getByText('tool')).toBeInTheDocument()
  })

  it('renders thought_chunk event type', () => {
    const events = [makeEvent({ type: 'thought_chunk', summary: 'thinking...' })]
    render(<ActivityFeed events={events} filterAgent={null} />)
    expect(screen.getByText('think')).toBeInTheDocument()
  })

  it('renders message_chunk event type', () => {
    const events = [makeEvent({ type: 'message_chunk', summary: 'hello' })]
    render(<ActivityFeed events={events} filterAgent={null} />)
    expect(screen.getByText('msg')).toBeInTheDocument()
  })

  it('renders unknown event type with default config', () => {
    const events = [makeEvent({ type: 'custom_type', summary: 'custom' })]
    render(<ActivityFeed events={events} filterAgent={null} />)
    expect(screen.getByText('event')).toBeInTheDocument()
  })
})
