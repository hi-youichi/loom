export type MessageRole = 'user' | 'assistant'

export type TextBlock = {
  id: string
  type: 'text'
  text: string
}

export type ToolStatus = 'queued' | 'running' | 'done' | 'error' | 'approval_required'

export type ToolBlock = {
  id: string
  type: 'tool'
  callId: string
  name: string
  status: ToolStatus
  argumentsText: string
  outputText: string
  resultText: string
  isError: boolean
}

export type MessageBlock = TextBlock | ToolBlock

export type Message = {
  id: string
  role: MessageRole
  blocks: MessageBlock[]
  createdAt: string
}
