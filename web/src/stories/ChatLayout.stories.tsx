import type { Meta, StoryObj } from '@storybook/react-vite'
import { ChatLayout } from '../components/layout/ChatLayout'

const meta = {
  title: 'Layout/ChatLayout',
  component: ChatLayout,
  parameters: { layout: 'centered' },
  tags: ['autodocs'],
} satisfies Meta<typeof ChatLayout>

export default meta
type Story = StoryObj<typeof meta>

export const Default: Story = {
  args: {
    children: (
      <>
        <div style={{ padding: 8, background: '#f5f5f5' }}>消息区域</div>
        <div style={{ padding: 8, background: '#eee' }}>输入区域</div>
      </>
    ),
  },
}

export const WithSingleChild: Story = {
  args: {
    children: <p>单个子节点</p>,
  },
}
