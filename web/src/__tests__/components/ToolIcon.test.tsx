import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import { ToolIcon } from '../../components/ToolIcon'

describe('ToolIcon', () => {
  it('renders a known icon', () => {
    const { container } = render(<ToolIcon name="file-text" />)
    expect(container.querySelector('svg')).toBeInTheDocument()
  })

  it('renders with custom size', () => {
    const { container } = render(<ToolIcon name="file-text" size={24} />)
    const svg = container.querySelector('svg')
    expect(svg).toHaveAttribute('width', '24')
    expect(svg).toHaveAttribute('height', '24')
  })

  it('renders with custom className', () => {
    const { container } = render(<ToolIcon name="file-text" className="test-class" />)
    const svg = container.querySelector('svg')
    expect(svg?.classList.contains('test-class') || svg?.parentElement?.classList.contains('test-class') || container.innerHTML.includes('test-class')).toBeTruthy()
  })

  it('returns null for unknown icon name', () => {
    const { container } = render(<ToolIcon name={'nonexistent-icon' as any} />)
    expect(container.querySelector('svg')).toBeNull()
  })

  it('renders all known icon names from ICON_MAP', () => {
    const icons = [
      'file-text', 'pencil', 'trash-2', 'folder-input', 'search',
      'play', 'brain', 'globe', 'settings', 'loader', 'check-circle',
      'x-circle', 'lock', 'hourglass', 'sun', 'moon', 'monitor',
    ] as const

    for (const name of icons) {
      const { container } = render(<ToolIcon name={name} />)
      expect(container.querySelector('svg')).toBeInTheDocument()
    }
  })
})
