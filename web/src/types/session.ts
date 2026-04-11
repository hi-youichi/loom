/**
 * Session types for the recent sessions feature
 */

/** Session status */
export type SessionStatus = 'active' | 'archived' | 'deleted'

/** Session sort options */
export type SessionSortBy = 'recent' | 'name' | 'messageCount' | 'updated'

/** Session group options */
export type SessionGroupBy = 'date' | 'agent' | 'model' | 'none'

/** Session action types */
export type SessionAction = 'rename' | 'delete' | 'archive' | 'pin' | 'export' | 'duplicate'

/** Session data structure */
export interface Session {
  id: string
  title: string                    // Session title (auto-generated or user-edited)
  createdAt: string               // Creation timestamp (ISO 8601)
  updatedAt: string               // Last update timestamp (ISO 8601)
  lastMessage: string             // Preview of the last message
  messageCount: number            // Total message count
  agent: string                   // Agent used (e.g., 'dev', 'ask')
  model: string                   // Model used (e.g., 'anthropic/claude-3.5-sonnet')
  workspace?: string              // Associated workspace
  tags?: string[]                 // User-defined tags
  isPinned: boolean               // Whether session is pinned
  isArchived: boolean             // Whether session is archived
  status: SessionStatus           // Session status
}

/** Session list filters */
export interface SessionFilters {
  searchQuery: string
  agent: string | null
  model: string | null
  tags: string[]
  dateRange?: {
    start: string
    end: string
  }
}

/** Session list options */
export interface SessionListOptions {
  sortBy: SessionSortBy
  groupBy: SessionGroupBy
  filters: SessionFilters
}
