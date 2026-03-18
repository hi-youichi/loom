import type { Meta, StoryObj } from '@storybook/react-vite'
import { fn } from 'storybook/test'
import { MessageComposer } from '../components/MessageComposer'

const meta = {
  title: 'Chat/MessageComposer',
  component: MessageComposer,
  parameters: { layout: 'centered' },
  tags: ['autodocs'],
  argTypes: {
    onSend: { action: 'send' },
  },
} satisfies Meta<typeof MessageComposer>

export default meta
type Story = StoryObj<typeof meta>

export const Default: Story = {
  args: {
    disabled: false,
    onSend: fn(),
  },
}

export const Disabled: Story = {
  args: {
    disabled: true,
    onSend: fn(),
  },
}
