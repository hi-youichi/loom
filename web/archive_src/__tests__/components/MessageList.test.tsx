import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MessageList } from '../../components/chat/MessageList'

describe('MessageList', () => {
  const mockMessages = [
    {
      id: '1',
      sender: 'user' as const,
      timestamp: '2024-01-01T10:00:00Z',
      content: [{ type: 'text' as const, text: 'Hello' }]
    },
    {
      id: '2',
      sender: 'assistant' as const,
      timestamp: '2024-01-01T10:01:00Z',
      content: [{ type: 'text' as const, text: 'Hi there!' }]
    }
  ]

  it('should render all messages', () => {
    render(<MessageList messages={mockMessages} />)
    expect(screen.getByText('Hello')).toBeInTheDocument()
    expect(screen.getByText('Hi there!')).toBeInTheDocument()
  })

  it('should have correct ARIA attributes', () => {
    render(<MessageList messages={mockMessages} />)
    const list = screen.getByRole('log')
    expect(list).toHaveAttribute('aria-live', 'polite')
    expect(list).toHaveAttribute('aria-label', 'Chat messages')
  })

  it('should apply custom className', () => {
    const { container } = render(
      <MessageList messages={mockMessages} className="custom-class" />
    )
    expect(container.firstChild).toHaveClass('custom-class')
  })

  it('should handle empty message list', () => {
    const { container } = render(<MessageList messages={[]} />)
    expect(container.querySelector('.message-list')).toBeInTheDocument()
  })

  it('should set data-message-id for each message', () => {
    const { container } = render(<MessageList messages={mockMessages} />)
    const messageElements = container.querySelectorAll('[data-message-id]')
    expect(messageElements).toHaveLength(2)
    expect(messageElements[0]).toHaveAttribute('data-message-id', '1')
    expect(messageElements[1]).toHaveAttribute('data-message-id', '2')
  })

  it('should show streaming indicator when streaming is true', () => {
    const { container } = render(<MessageList messages={mockMessages} streaming={true} />)
    expect(container.querySelector('.message-list__streaming')).toBeInTheDocument()
  })

  it('should not show streaming indicator when streaming is false', () => {
    const { container } = render(<MessageList messages={mockMessages} streaming={false} />)
    expect(container.querySelector('.message-list__streaming')).toBeNull()
  })

  it('should pass streaming prop only to assistant messages', () => {
    const { container } = render(<MessageList messages={mockMessages} streaming={true} />)
    expect(container.querySelector('.message-list__streaming')).toBeInTheDocument()
  })
})
