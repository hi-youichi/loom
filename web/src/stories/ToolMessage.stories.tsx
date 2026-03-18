import type { Meta, StoryObj } from '@storybook/react-vite'
import { ToolMessage } from '../components/chat/ToolMessage'
import type { UIToolContent } from '../types/ui/message'

const meta = {
  title: 'Chat/ToolMessage',
  component: ToolMessage,
  parameters: { layout: 'centered' },
  tags: ['autodocs'],
  argTypes: {
    content: { control: false },
  },
} satisfies Meta<typeof ToolMessage>

export default meta
type Story = StoryObj<typeof meta>

const baseTool: UIToolContent = {
  type: 'tool',
  id: 'call-1',
  name: 'get_weather',
  status: 'success',
  argumentsText: '{"location": "Beijing"}',
  outputText: '',
  resultText: '{"temp": 25, "condition": "sunny"}',
  isError: false,
}

export const Pending: Story = {
  args: {
    content: { ...baseTool, status: 'pending', argumentsText: '', outputText: '', resultText: '' },
  },
}

export const Running: Story = {
  args: {
    content: {
      ...baseTool,
      status: 'running',
      outputText: 'Fetching weather data...',
      resultText: '',
    },
  },
}

export const Success: Story = {
  args: {
    content: {
      ...baseTool,
      status: 'success',
      outputText: 'Data received.',
      resultText: '{"temp": 25, "condition": "sunny"}',
    },
  },
}

export const Error: Story = {
  args: {
    content: {
      ...baseTool,
      status: 'error',
      isError: true,
      resultText: 'API rate limit exceeded',
    },
  },
}

export const WithAllSections: Story = {
  args: {
    content: {
      ...baseTool,
      argumentsText: '{"query": "search term", "limit": 10}',
      outputText: 'Searching... 5 results found.',
      resultText: '{"results": []}',
    },
  },
}
