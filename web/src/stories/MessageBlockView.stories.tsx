import type { Meta, StoryObj } from '@storybook/react-vite'
import { MessageBlockView } from '../components/chat/MessageBlockView'
import type { UITextContent, UIToolContent } from '../types/ui/message'

const meta = {
  title: 'Chat/MessageBlockView',
  component: MessageBlockView,
  parameters: { layout: 'centered' },
  tags: ['autodocs'],
  argTypes: { content: { control: false } },
} satisfies Meta<typeof MessageBlockView>

export default meta
type Story = StoryObj<typeof meta>

const textContent: UITextContent = {
  type: 'text',
  text: 'MessageBlockView 会根据 content 类型自动渲染 TextMessage 或 ToolMessage。',
}

const toolContent: UIToolContent = {
  type: 'tool',
  id: 'call-1',
  name: 'example_tool',
  status: 'success',
  argumentsText: '{}',
  outputText: 'output',
  resultText: 'result',
  isError: false,
}

export const TextBlock: Story = {
  args: { content: textContent },
}

export const ToolBlock: Story = {
  args: { content: toolContent },
}
