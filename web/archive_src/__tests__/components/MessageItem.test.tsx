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

  it('should render user text message', () => {
    render(<MessageItem {...defaultProps} />)
    expect(screen.getByText('Hello World')).toBeInTheDocument()
  })

  it('should render assistant text message', () => {
    render(<MessageItem {...defaultProps} sender="assistant" />)
    expect(screen.getByText('Hello World')).toBeInTheDocument()
  })

  it('should have correct ARIA label for user message', () => {
    render(<MessageItem {...defaultProps} />)
    const article = screen.getByRole('article')
    expect(article).toHaveAttribute('aria-label', 'User message')
  })

  it('should have correct ARIA label for assistant message', () => {
    render(<MessageItem {...defaultProps} sender="assistant" />)
    const article = screen.getByRole('article')
    expect(article).toHaveAttribute('aria-label', 'Assistant message')
  })

  it('should apply custom className', () => {
    const { container } = render(
      <MessageItem {...defaultProps} className="custom-class" />
    )
    const article = container.querySelector('article')
    expect(article).toHaveClass('custom-class')
  })

  it('should show retry button for user message with onRetry', () => {
    const onRetry = vi.fn()
    render(<MessageItem {...defaultProps} onRetry={onRetry} />)
    const retryButton = screen.getByRole('button', { name: /Retry/i })
    expect(retryButton).toBeInTheDocument()
  })

  it('should not show retry button for assistant message', () => {
    const onRetry = vi.fn()
    render(<MessageItem {...defaultProps} sender="assistant" onRetry={onRetry} />)
    const retryButton = screen.queryByRole('button', { name: /Retry/i })
    expect(retryButton).not.toBeInTheDocument()
  })

  it('should not show retry button when onRetry is not provided', () => {
    render(<MessageItem {...defaultProps} />)
    const retryButton = screen.queryByRole('button', { name: /Retry/i })
    expect(retryButton).not.toBeInTheDocument()
  })

  it('should render tool content via ToolCard', () => {
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

    const toolCard = screen.getByRole('article')
    expect(toolCard).toHaveAttribute('aria-label', expect.stringContaining('test-tool'))
  })

  it('should render tool content with error status', () => {
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

    const toolCard = screen.getByRole('article')
    expect(toolCard).toHaveAttribute('aria-label', expect.stringContaining('failing-tool'))
  })

  it('should render multiple text content blocks', () => {
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

  it('should set data-message-id on article', () => {
    render(<MessageItem {...defaultProps} />)
    const article = screen.getByRole('article')
    expect(article).toHaveAttribute('data-message-id', '1')
  })

  it('should call onRetry when retry button is clicked', () => {
    const onRetry = vi.fn()
    render(<MessageItem {...defaultProps} onRetry={onRetry} />)
    screen.getByRole('button', { name: /Retry/i }).click()
    expect(onRetry).toHaveBeenCalledTimes(1)
  })
})
