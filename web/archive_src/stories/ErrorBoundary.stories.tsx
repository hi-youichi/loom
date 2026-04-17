import type { Meta, StoryObj } from '@storybook/react-vite'
import { ChatErrorBoundary } from '../components/error/ErrorBoundary'

const meta = {
  title: 'Error/ErrorBoundary',
  component: ChatErrorBoundary,
  parameters: { layout: 'centered' },
  tags: ['autodocs'],
} satisfies Meta<typeof ChatErrorBoundary>

export default meta
type Story = StoryObj<typeof meta>

function Thrower() {
  throw new Error('Storybook 测试错误')
  return null
}

export const Normal: Story = {
  args: {
    children: <p>子组件正常渲染时不会显示错误 UI。</p>,
  },
}

export const CustomFallback: Story = {
  args: {
    children: <Thrower />,
    fallback: (
      <div style={{ padding: 16, border: '1px solid #f44336', borderRadius: 8 }}>
        <strong>自定义错误展示</strong>
        <p>可传入 fallback 替换默认错误界面。</p>
      </div>
    ),
  },
}

export const WithErrorTrigger: Story = {
  args: {
    children: <Thrower />,
  },
}
