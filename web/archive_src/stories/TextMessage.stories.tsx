import type { Meta, StoryObj } from '@storybook/react-vite'
import { TextMessage } from '../components/chat/TextMessage'
import type { UITextContent } from '../types/ui/message'

const meta = {
  title: 'Chat/TextMessage',
  component: TextMessage,
  parameters: { layout: 'centered' },
  tags: ['autodocs'],
  argTypes: {
    content: { control: false },
  },
} satisfies Meta<typeof TextMessage>

export default meta
type Story = StoryObj<typeof meta>

const defaultContent: UITextContent = {
  type: 'text',
  text: '这是一条文本消息内容。',
}

export const Default: Story = {
  args: { content: defaultContent },
}

export const LongText: Story = {
  args: {
    content: {
      type: 'text',
      text: '这是一段较长的文本。可以包含多行或重复内容，用于测试组件在长文本下的展示效果。\n\n第二段内容。',
    },
  },
}

export const WithClassName: Story = {
  args: {
    content: defaultContent,
    className: 'custom-class',
  },
}
