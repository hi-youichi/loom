import type { Meta, StoryObj } from '@storybook/react-vite'
import { ThinkIndicator } from '../components/ThinkIndicator'

const meta = {
  title: 'Chat/ThinkIndicator',
  component: ThinkIndicator,
  parameters: { layout: 'centered' },
  tags: ['autodocs'],
} satisfies Meta<typeof ThinkIndicator>

export default meta
type Story = StoryObj<typeof meta>

export const Idle: Story = {
  args: {
    lines: ['First thought line.', 'Second line.'],
    active: false,
  },
}

export const Active: Story = {
  args: {
    lines: ['Reasoning step 1...', 'Reasoning step 2...'],
    active: true,
  },
}

export const EmptyIdle: Story = {
  args: {
    lines: [],
    active: false,
  },
}

export const LongLines: Story = {
  args: {
    lines: [
      'First reasoning step with some longer content.',
      'Second step.',
      'Third step with more detail.',
    ],
    active: true,
  },
}
