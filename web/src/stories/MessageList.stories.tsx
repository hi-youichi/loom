import type { Meta, StoryObj } from '@storybook/react-vite'
import { MessageList } from '../components/chat/MessageList'
import type { UIMessageItemProps } from '../types/ui/message'

const meta = {
  title: 'Chat/MessageList',
  component: MessageList,
  parameters: {
    layout: 'centered',
    viewport: { defaultViewport: 'mobile1' },
  },
  tags: ['autodocs'],
  decorators: [
    (Story) => (
      <div style={{ width: 360, height: 400, border: '1px solid #eee', overflow: 'hidden' }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof MessageList>

export default meta
type Story = StoryObj<typeof meta>

const now = new Date().toISOString()

const singleMessage: UIMessageItemProps[] = [
  {
    id: '1',
    sender: 'user',
    timestamp: now,
    content: [{ type: 'text', text: '你好' }],
  },
]

const conversation: UIMessageItemProps[] = [
  {
    id: '1',
    sender: 'user',
    timestamp: now,
    content: [{ type: 'text', text: '今天天气怎么样？' }],
  },
  {
    id: '2',
    sender: 'assistant',
    timestamp: now,
    content: [
      { type: 'text', text: '正在查询天气。' },
      {
        type: 'tool',
        id: 'c1',
        name: 'get_weather',
        status: 'success',
        argumentsText: '{}',
        outputText: '',
        resultText: '{"temp": 25}',
        isError: false,
      },
    ],
  },
  {
    id: '3',
    sender: 'user',
    timestamp: now,
    content: [{ type: 'text', text: '谢谢' }],
  },
]

export const Empty: Story = {
  args: { messages: [] },
}

export const SingleMessage: Story = {
  args: { messages: singleMessage },
}

export const Conversation: Story = {
  args: { messages: conversation },
}
