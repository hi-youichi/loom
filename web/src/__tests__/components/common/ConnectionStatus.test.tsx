import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { ConnectionStatus } from '../../../components/common/ConnectionStatus'
import type { WebSocketStatus } from '../../../types/protocol/loom'

describe('ConnectionStatus', () => {
  const statuses: WebSocketStatus[] = ['connecting', 'connected', 'disconnected', 'error']
  
  statuses.forEach((status) => {
    it(`应该正确渲染 ${status} 状态`, () => {
      const { container } = render(<ConnectionStatus status={status} />)
      
      expect(container.firstChild).toBeInTheDocument()
      expect(screen.getByRole('status')).toBeInTheDocument()
    })
  })

  it('应该应用自定义className', () => {
    const { container } = render(
      <ConnectionStatus status="connected" className="custom-class" />
    )
    
    expect(container.firstChild).toHaveClass('custom-class')
  })

  it('应该有正确的aria标签', () => {
    render(<ConnectionStatus status="connected" />)
    
    const statusElement = screen.getByRole('status')
    expect(statusElement).toHaveAttribute('aria-live', 'polite')
  })
})
