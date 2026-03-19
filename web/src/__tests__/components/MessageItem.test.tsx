import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MessageItem } from '../../components/chat/MessageItem'
import type { UIMessageItemProps } from '../../../types/ui/message'

describe('MessageItem', () => {
  const defaultProps: UIMessageItemProps = {
    id: '1',
    sender: 'user',
    timestamp: '2024-01-01T10:30:00Z',
    content: [{ type: 'text', text: 'Hello World' }],
  }

  it('应该渲染用户消息', () => {
    render(<MessageItem {...defaultProps} />)
    
    expect(screen.getByText('Hello World')).toBeInTheDocument()
    expect(screen.getByText('用户')).toBeInTheDocument()
  })

  it('应该渲染助手消息', () => {
    render(<MessageItem {...defaultProps} sender="assistant" />)
    
    expect(screen.getByText('Hello World')).toBeInTheDocument()
    expect(screen.getByText('助手')).toBeInTheDocument()
  })

  it('应该显示时间戳', () => {
    render(<MessageItem {...defaultProps} />)
    
    // 时间格式化后应该显示
    const timeElement = screen.getByRole('time')
    expect(timeElement).toBeInTheDocument()
    expect(timeElement).toHaveAttribute('dateTime', '2024-01-01T10:30:00Z')
  })

  it('应该应用自定义className', () => {
    const { container } = render(
      <MessageItem {...defaultProps} className="custom-class" />
    )
    
    const article = container.querySelector('article')
    expect(article).toHaveClass('custom-class')
  })

  it('应该为用户消息显示重试按钮', () => {
    const onRetry = vi.fn()
    render(<MessageItem {...defaultProps} onRetry={onRetry} />)
    
    const retryButton = screen.getByRole('button', { name: /重试/i })
    expect(retryButton).toBeInTheDocument()
  })

  it('应该为助手消息不显示重试按钮', () => {
    const onRetry = vi.fn()
    render(
      <MessageItem 
        {...defaultProps} 
        sender="assistant" 
        onRetry={onRetry} 
      />
    )
    
    const retryButton = screen.queryByRole('button', { name: /重试/i })
    expect(retryButton).not.toBeInTheDocument()
  })

  it('应该渲染工具内容', () => {
    const props: UIMessageItemProps = {
      ...defaultProps,
      content: [{
        type: 'tool',
        id: 'tool-1',
        name: 'test-tool',
        status: 'success',
        argumentsText: '{"arg": "value"}',
        outputText: 'output',
        resultText: 'result',
        isError: false,
      }],
    }
    
    render(<MessageItem {...props} />)
    
    expect(screen.getByText('test-tool')).toBeInTheDocument()
    expect(screen.getByText('success')).toBeInTheDocument()
  })

  it('应该渲染多个内容块', () => {
    const props: UIMessageItemProps = {
      ...defaultProps,
      content: [
        { type: 'text', text: 'First' },
        { type: 'text', text: 'Second' },
      ],
    }
    
    render(<MessageItem {...props} />)
    
    expect(screen.getByText('First')).toBeInTheDocument()
    expect(screen.getByText('Second')).toBeInTheDocument()
  })

  it('应该有正确的ARIA标签', () => {
    render(<MessageItem {...defaultProps} />)
    
    const article = screen.getByRole('article')
    expect(article).toHaveAttribute('aria-label', '用户消息')
  })

  it('应该处理工具错误状态', () => {
    const props: UIMessageItemProps = {
      ...defaultProps,
      content: [{
        type: 'tool',
        id: 'tool-1',
        name: 'failing-tool',
        status: 'error',
        argumentsText: '{"arg": "value"}',
        outputText: 'error output',
        resultText: 'error result',
        isError: true,
      }],
    }
    
    render(<MessageItem {...props} />)
    
    expect(screen.getByText('failing-tool')).toBeInTheDocument()
    expect(screen.getByText('error')).toBeInTheDocument()
    expect(screen.getByText('错误:')).toBeInTheDocument()
    expect(screen.getByText('error result')).toBeInTheDocument()
  })
})
