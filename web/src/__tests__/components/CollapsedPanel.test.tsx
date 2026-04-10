import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { CollapsedPanel } from '../../components/chat/CollapsedPanel'

describe('CollapsedPanel', () => {
  it('should render with unread count', () => {
    render(<CollapsedPanel unreadCount={3} onExpand={() => {}} />)
    expect(screen.getByText('3')).toBeInTheDocument()
  })

  it('should render with zero unread count', () => {
    const { container } = render(
      <CollapsedPanel unreadCount={0} onExpand={() => {}} />
    )

    const button = container.querySelector('button')
    expect(button).toBeInTheDocument()
    expect(button?.getAttribute('aria-label')).toContain('0')
  })

  it('should call onExpand when clicked', () => {
    const handleExpand = vi.fn()
    const { container } = render(
      <CollapsedPanel unreadCount={5} onExpand={handleExpand} />
    )

    const button = container.querySelector('button')!
    button.click()

    expect(handleExpand).toHaveBeenCalledTimes(1)
  })

  it('should have correct aria-label', () => {
    const { container } = render(
      <CollapsedPanel unreadCount={7} onExpand={() => {}} />
    )

    const button = container.querySelector('button')
    expect(button?.getAttribute('aria-label')).toContain('7')
  })

  it('should not show count span when unreadCount is 0', () => {
    const { container } = render(
      <CollapsedPanel unreadCount={0} onExpand={() => {}} />
    )
    expect(container.querySelector('span')).toBeNull()
  })

  it('should show count span when unreadCount > 0', () => {
    const { container } = render(
      <CollapsedPanel unreadCount={2} onExpand={() => {}} />
    )
    expect(container.querySelector('span')?.textContent).toBe('2')
  })
})
