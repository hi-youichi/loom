import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MessageList } from '../components/chat/MessageList'

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

  it('应该渲染所有消息', () => {
    render(<MessageList messages={mockMessages} />)
    
    expect(screen.getByText('Hello')).toBeInTheDocument()
    expect(screen.getByText('Hi there!')).toBeInTheDocument()
  })

  it('应该有正确的ARIA属性', () => {
    render(<MessageList messages={mockMessages} />)
    
    const list = screen.getByRole('log')
    expect(list).toHaveAttribute('aria-live', 'polite')
    expect(list).toHaveAttribute('aria-label', '聊天消息')
  })

  it('应该应用自定义className', () => {
    const { container } = render(
      <MessageList messages={mockMessages} className="custom-class" />
    )
    
    expect(container.firstChild).toHaveClass('custom-class')
  })

  it('应该处理空消息列表', () => {
    const { container } = render(<MessageList messages={[]} />)
    
    expect(container.querySelector('.message-list')).toBeInTheDocument()
    expect(container.querySelectorAll('.message-item')).toHaveLength(0)
  })

  it('应该为每个消息设置正确的key', () => {
    const { container } = render(<MessageList messages={mockMessages} />)
    
    const messageElements = container.querySelectorAll('[data-message-id]')
    expect(messageElements).toHaveLength(2)
    expect(messageElements[0]).toHaveAttribute('data-message-id', '1')
    expect(messageElements[1]).toHaveAttribute('data-message-id', '2')
  })
})
