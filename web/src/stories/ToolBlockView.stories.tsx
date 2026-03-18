import type { Meta, StoryObj } from '@storybook/react-vite'
import { ToolBlockView } from '../components/ToolBlockView'
import type { ToolBlock } from '../types/chat'

const meta = {
  title: 'Chat/ToolBlockView',
  component: ToolBlockView,
  parameters: { layout: 'centered' },
  tags: ['autodocs'],
  argTypes: { tool: { control: false } },
} satisfies Meta<typeof ToolBlockView>

export default meta
type Story = StoryObj<typeof meta>

const baseTool: ToolBlock = {
  id: 'tool-1',
  type: 'tool',
  callId: 'call-1',
  name: 'get_weather',
  status: 'done',
  argumentsText: '{}',
  outputText: '',
  resultText: '',
  isError: false,
}

export const Queued: Story = {
  args: {
    tool: { ...baseTool, status: 'queued' },
    defaultExpanded: true,
  },
}

export const Running: Story = {
  args: {
    tool: {
      ...baseTool,
      status: 'running',
      outputText: 'Fetching...',
    },
    defaultExpanded: true,
  },
}

export const Done: Story = {
  args: {
    tool: {
      ...baseTool,
      status: 'done',
      argumentsText: '{"location": "Beijing"}',
      outputText: 'Data received.',
      resultText: '{"temp": 25, "condition": "sunny"}',
    },
    defaultExpanded: true,
  },
}

export const Error: Story = {
  args: {
    tool: {
      ...baseTool,
      status: 'error',
      isError: true,
      resultText: 'API rate limit exceeded',
    },
    defaultExpanded: true,
  },
}

export const Collapsed: Story = {
  args: {
    tool: {
      ...baseTool,
      argumentsText: '{"q": "test"}',
      outputText: 'output',
      resultText: 'result',
    },
    defaultExpanded: false,
  },
}
