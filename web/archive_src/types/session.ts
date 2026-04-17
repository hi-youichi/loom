export type SessionStatus = 'active' | 'archived' | 'deleted'

export interface Session {
  id: string
  title: string
  createdAt: string
  updatedAt: string
  lastMessage: string
  messageCount: number
  agent: string
  model: string
  workspace?: string
  tags?: string[]
  isPinned: boolean
  isArchived?: boolean
  status?: SessionStatus
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

export interface SessionListOptions {
  sortBy?: SessionSort
  groupBy?: 'none' | 'date' | 'agent' | 'model'
  filters?: SessionFilter
}
