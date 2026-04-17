import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import { MarkdownContent } from '../../components/chat/MarkdownContent'

describe('MarkdownContent', () => {
  it('renders markdown text', () => {
    const { container } = render(<MarkdownContent text="Hello **world**" />)
    expect(container.querySelector('.markdown-body')).toBeInTheDocument()
    expect(container.innerHTML).toContain('Hello')
  })

  it('returns null for empty text', () => {
    const { container } = render(<MarkdownContent text="" />)
    expect(container.querySelector('.markdown-body')).toBeNull()
  })

  it('applies custom className', () => {
    const { container } = render(<MarkdownContent text="test" className="custom" />)
    expect(container.querySelector('.markdown-body.custom')).toBeInTheDocument()
  })

  it('closes incomplete code fence when streaming', () => {
    const { container } = render(<MarkdownContent text="```js\nconsole.log" streaming={true} />)
    expect(container.querySelector('.markdown-body')).toBeInTheDocument()
  })

  it('does not modify complete code fence when streaming', () => {
    const { container } = render(<MarkdownContent text="```js\ncode\n```" streaming={true} />)
    expect(container.querySelector('.markdown-body')).toBeInTheDocument()
  })

  it('does not modify text when not streaming', () => {
    const { container } = render(<MarkdownContent text="```js\nconsole.log" streaming={false} />)
    expect(container.querySelector('.markdown-body')).toBeInTheDocument()
  })

  it('renders GFM tables', () => {
    const markdown = '| a | b |\n|---|---|\n| 1 | 2 |'
    const { container } = render(<MarkdownContent text={markdown} />)
    expect(container.querySelector('table')).toBeInTheDocument()
  })
})
