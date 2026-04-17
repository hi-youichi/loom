import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import { ModelSelector } from '../../components/ModelSelector'

const mockModels = [
  { id: 'gpt-4o', name: 'GPT-4o', provider: 'OpenAI' },
  { id: 'claude-3-5-sonnet', name: 'Claude 3.5 Sonnet', provider: 'Anthropic' },
  { id: 'gpt-4o-mini', name: 'GPT-4o Mini', provider: 'OpenAI' },
]

vi.mock('../../hooks/useModels', () => ({
  useModels: () => ({
    models: mockModels,
    loading: false,
    error: null,
    refetch: vi.fn(),
  }),
}))

describe('ModelSelector', () => {
  it('renders trigger with placeholder when no model selected', () => {
    render(<ModelSelector />)
    expect(screen.getByText('Select model...')).toBeInTheDocument()
  })

  it('renders selected model name', () => {
    render(<ModelSelector value="claude-3-5-sonnet" />)
    expect(screen.getByText('Claude 3.5 Sonnet')).toBeInTheDocument()
  })

  it('renders a chevron icon inside the trigger', () => {
    const { container } = render(<ModelSelector />)
    const trigger = container.querySelector('.model-selector__trigger')
    expect(trigger).toBeInTheDocument()
    const svg = trigger?.querySelector('svg')
    expect(svg).toBeInTheDocument()
  })

  it('shows loading spinner when loading and no model selected', async () => {
    vi.doMock('../../hooks/useModels', () => ({
      useModels: () => ({
        models: [] as unknown[],
        loading: true,
        error: null as string | null,
        refetch: vi.fn(),
      }),
    }))

    vi.resetModules()

    const { ModelSelector: FreshSelector } = await import('../../components/ModelSelector')
    const { container } = render(<FreshSelector />)
    const spinner = container.querySelector('.animate-spin')
    expect(spinner).toBeInTheDocument()

    vi.doUnmock('../../hooks/useModels')
    vi.resetModules()
  })

  it('does not show spinner when loading but a model is selected via value', () => {
    vi.doMock('../../hooks/useModels', () => ({
      useModels: () => ({
        models: [] as unknown[],
        loading: true,
        error: null as string | null,
        refetch: vi.fn(),
      }),
    }))

    const { container } = render(<ModelSelector value="gpt-4o" />)
    expect(screen.getByText('GPT-4o')).toBeInTheDocument()
    const spinner = container.querySelector('.animate-spin')
    expect(spinner).not.toBeInTheDocument()
  })

  it('has a wrapper with model-selector class', () => {
    const { container } = render(<ModelSelector />)
    expect(container.querySelector('.model-selector')).toBeInTheDocument()
  })

  it('passes className to wrapper', () => {
    const { container } = render(<ModelSelector className="custom-class" />)
    expect(container.querySelector('.model-selector')).toHaveClass('custom-class')
  })

  it('trigger has correct styling classes', () => {
    const { container } = render(<ModelSelector />)
    const trigger = container.querySelector('.model-selector__trigger')
    expect(trigger).toHaveClass('border')
    expect(trigger).toHaveClass('bg-background')
  })

  it('uses border-border class instead of border-input on trigger', () => {
    const { container } = render(<ModelSelector />)
    const trigger = container.querySelector('.model-selector__trigger')
    expect(trigger?.className).toContain('border-border')
    expect(trigger?.className).not.toContain('border-input')
  })
})
