export interface Session {
  id: string                    // threadId
  title: string                 // 会话标题
  createdAt: string
  updatedAt: string
  lastMessage: string           // 最后一条用户消息
  messageCount: number
  agent: string
  model: string
  workspace?: string
  tags?: string[]
  isPinned: boolean
  isArchived?: boolean
}

export type SessionAction = 
  | 'rename' 
  | 'delete' 
  | 'archive' 
  | 'pin' 
  | 'export'
  | 'view'

export interface SessionFilter {
  searchQuery?: string
  agent?: string | null
  model?: string | null
  dateRange?: {
    start: Date
    end: Date
  }
  tags?: string[]
}

export type SessionSort = 'recent' | 'name' | 'messageCount' | 'updated'
