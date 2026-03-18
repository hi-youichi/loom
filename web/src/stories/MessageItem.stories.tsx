import type { Meta, StoryObj } from '@storybook/react-vite'
import { fn } from 'storybook/test'
import { MessageItem } from '../components/chat/MessageItem'
import type { UIMessageItemProps } from '../types/ui/message'

const meta = {
  title: 'Chat/MessageItem',
  component: MessageItem,
  parameters: { layout: 'centered' },
  tags: ['autodocs'],
  argTypes: { onRetry: { action: 'retry' } },
} satisfies Meta<typeof MessageItem>

export default meta
type Story = StoryObj<typeof meta>

const now = new Date().toISOString()

export const UserText: Story = {
  args: {
    id: 'msg-1',
    sender: 'user',
    timestamp: now,
    content: [{ type: 'text', text: '用户发送的文本消息' }],
  },
}

export const AssistantText: Story = {
  args: {
    id: 'msg-2',
    sender: 'assistant',
    timestamp: now,
    content: [{ type: 'text', text: '助手回复的文本内容。' }],
  },
}

export const AssistantWithTool: Story = {
  args: {
    id: 'msg-3',
    sender: 'assistant',
    timestamp: now,
    content: [
      { type: 'text', text: '正在调用工具获取数据。' },
      {
        type: 'tool',
        id: 'call-1',
        name: 'get_weather',
        status: 'success',
        argumentsText: '{"location": "Beijing"}',
        outputText: '',
        resultText: '{"temp": 25}',
        isError: false,
      },
    ],
  },
}

export const UserWithRetry: Story = {
  args: {
    id: 'msg-4',
    sender: 'user',
    timestamp: now,
    content: [{ type: 'text', text: '发送失败可重试的消息' }],
    onRetry: fn(),
  },
}
