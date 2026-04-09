import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import { CollapsedPanel } from '../../components/chat/CollapsedPanel'

describe('CollapsedPanel', () => {
  it('should render with unread count', () => {
    const { getByText } = render(
      <CollapsedPanel unreadCount={3} onExpand={() => {}} />
    )

    expect(getByText('3')).toBeInTheDocument()
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
    const { getByText } = render(
      <CollapsedPanel unreadCount={5} onExpand={handleExpand} />
    )

    const button = getByText('💬')
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
})
