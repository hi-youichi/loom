import { useEffect, useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'
import { fn } from 'storybook/test'
import { ChatErrorBoundary } from '../components/error/ErrorBoundary'
import { ChatLayout } from '../components/layout/ChatLayout'
import { MessageList } from '../components/chat/MessageList'
import { ThinkIndicator } from '../components/ThinkIndicator'
import { MessageComposer } from '../components/MessageComposer'
import type { UIMessageItemProps } from '../types/ui/message'

const now = new Date().toISOString()

function ts(offset = 0) {
  return new Date(Date.now() + offset).toISOString()
}

/**
 * 有节奏的完整聊天演示：按时间轴自动展示用户消息 → Thinking → 助手回复（含工具）
 */
function RhythmicChatDemo() {
  const [messages, setMessages] = useState<UIMessageItemProps[]>([
    { id: '1', sender: 'user', timestamp: now, content: [{ type: 'text', text: '北京现在多少度？' }] },
  ])
  const [thinkingLines, setThinkingLines] = useState<string[]>([])
  const [thinkingActive, setThinkingActive] = useState(false)
  const [composerDisabled, setComposerDisabled] = useState(false)

  useEffect(() => {
    const delays = {
      step: 1800,
      thinkLine: 1600,
    }

    const t1 = setTimeout(() => {
      setThinkingLines(['分析用户问题：天气查询请求'])
      setThinkingActive(true)
      setComposerDisabled(true)
    }, delays.step)

    const t2 = setTimeout(() => {
      setThinkingLines((prev) => [...prev, '选择工具：get_weather'])
    }, delays.step + delays.thinkLine)

    const t3 = setTimeout(() => {
      setThinkingLines((prev) => [...prev, '构造参数并调用 API…'])
    }, delays.step + delays.thinkLine * 2)

    const t4 = setTimeout(() => {
      setThinkingLines([])
      setThinkingActive(false)
      setComposerDisabled(false)
      setMessages((prev) => [
        ...prev,
        {
          id: '2',
          sender: 'assistant',
          timestamp: ts(),
          content: [
            { type: 'text', text: '正在查询北京天气。' },
            {
              type: 'tool',
              id: 'call-1',
              name: 'get_weather',
              status: 'success',
              argumentsText: '{"location": "Beijing"}',
              outputText: 'Fetched from API.',
              resultText: '{"temp": 25, "condition": "sunny", "humidity": 40}',
              isError: false,
            },
          ],
        },
      ])
    }, delays.step + delays.thinkLine * 3 + 400)

    const t5 = setTimeout(() => {
      setMessages((prev) => [
        ...prev,
        {
          id: '3',
          sender: 'assistant',
          timestamp: ts(1000),
          content: [{ type: 'text', text: '北京当前气温约 25°C，晴，湿度 40%。' }],
        },
      ])
    }, delays.step + delays.thinkLine * 3 + 400 + 1200)

    return () => {
      clearTimeout(t1)
      clearTimeout(t2)
      clearTimeout(t3)
      clearTimeout(t4)
      clearTimeout(t5)
    }
  }, [])

  return (
    <ChatErrorBoundary>
      <ChatLayout>
        <MessageList messages={messages} />
        {thinkingLines.length > 0 ? (
          <ThinkIndicator lines={thinkingLines} active={thinkingActive} />
        ) : null}
        <MessageComposer disabled={composerDisabled} onSend={fn()} />
      </ChatLayout>
    </ChatErrorBoundary>
  )
}

const meta = {
  title: 'Demo/FullChatDemo',
  component: RhythmicChatDemo,
  parameters: {
    layout: 'centered',
    docs: {
      description: {
        story: '有节奏的完整聊天演示：自动依次展示用户消息 → Thinking（思考中）→ 助手回复与工具调用。',
      },
    },
  },
  tags: ['autodocs'],
  decorators: [
    (Story) => (
      <div
        className="shell"
        style={{
          width: 'min(820px, calc(100% - 32px))',
          height: '80vh',
          minHeight: 400,
          margin: '0 auto',
        }}
      >
        <section
          className="chat-panel"
          style={{ height: '100%', display: 'flex', flexDirection: 'column', gap: 18 }}
        >
          <Story />
        </section>
      </div>
    ),
  ],
} satisfies Meta<typeof RhythmicChatDemo>

export default meta
type Story = StoryObj<typeof meta>

export const Default: Story = {}
