export type MessageRole = 'user' | 'assistant'

export type Message = {
  id: string
  role: MessageRole
  content: string
  createdAt: string
}
