import type { Session } from '@loom/types'

const SESSIONS_STORAGE_KEY = 'loom-sessions'
const ACTIVE_SESSION_KEY = 'loom-active-session'

export class SessionService {
  static async listSessions(): Promise<Session[]> {
    try {
      const stored = localStorage.getItem(SESSIONS_STORAGE_KEY)
      if (!stored) return []

      const sessions: Session[] = JSON.parse(stored)
      return sessions.filter(s => s.status !== 'deleted')
    } catch (error) {
      console.error('Failed to load sessions:', error)
      return []
    }
  }

  static async getSession(id: string): Promise<Session | null> {
    const sessions = await this.listSessions()
    return sessions.find(s => s.id === id) || null
  }

  static async createSession(data: Partial<Session> = {}): Promise<Session> {
    const now = new Date().toISOString()
    const newSession: Session = {
      id: crypto.randomUUID(),
      title: data.title || this.generateDefaultTitle(data.lastMessage || '新对话'),
      createdAt: now,
      updatedAt: now,
      lastMessage: data.lastMessage || '',
      messageCount: data.messageCount || 0,
      agent: data.agent || 'dev',
      model: data.model || 'anthropic/claude-3-5-sonnet-20241022',
      workspace: data.workspace,
      tags: data.tags || [],
      isPinned: false,
      isArchived: false,
      status: 'active',
    }

    const sessions = await this.listSessions()
    sessions.unshift(newSession)
    this.saveToStorage(sessions)

    return newSession
  }

  static async updateSession(id: string, updates: Partial<Session>): Promise<Session> {
    const sessions = await this.listSessions()
    const index = sessions.findIndex(s => s.id === id)

    if (index === -1) {
      throw new Error(`Session not found: ${id}`)
    }

    const updatedSession: Session = {
      ...sessions[index],
      ...updates,
      id,
      updatedAt: new Date().toISOString(),
    }

    sessions[index] = updatedSession
    this.saveToStorage(sessions)

    return updatedSession
  }

  static async deleteSession(id: string): Promise<void> {
    const sessions = await this.listSessions()
    const filtered = sessions.filter(s => s.id !== id)

    if (filtered.length === sessions.length) {
      throw new Error(`Session not found: ${id}`)
    }

    this.saveToStorage(filtered)
  }

  static async softDeleteSession(id: string): Promise<void> {
    await this.updateSession(id, { status: 'deleted' })
  }

  static async togglePin(id: string): Promise<Session> {
    const session = await this.getSession(id)
    if (!session) throw new Error(`Session not found: ${id}`)

    return this.updateSession(id, { isPinned: !session.isPinned })
  }

  static async toggleArchive(id: string): Promise<Session> {
    const session = await this.getSession(id)
    if (!session) throw new Error(`Session not found: ${id}`)

    return this.updateSession(id, {
      isArchived: !session.isArchived,
      status: session.isArchived ? 'active' : 'archived',
    })
  }

  static async addMessage(sessionId: string, message: string): Promise<Session> {
    const session = await this.getSession(sessionId)
    if (!session) throw new Error(`Session not found: ${sessionId}`)

    return this.updateSession(sessionId, {
      lastMessage: message,
      messageCount: session.messageCount + 1,
    })
  }

  static async renameSession(id: string, title: string): Promise<Session> {
    return this.updateSession(id, { title })
  }

  static async searchSessions(query: string): Promise<Session[]> {
    const sessions = await this.listSessions()
    const lowerQuery = query.toLowerCase()

    return sessions.filter(session =>
      session.title.toLowerCase().includes(lowerQuery) ||
      session.lastMessage.toLowerCase().includes(lowerQuery) ||
      session.agent.toLowerCase().includes(lowerQuery) ||
      session.tags?.some(tag => tag.toLowerCase().includes(lowerQuery))
    )
  }

  static async getActiveSession(): Promise<string | null> {
    return localStorage.getItem(ACTIVE_SESSION_KEY)
  }

  static async setActiveSession(id: string): Promise<void> {
    localStorage.setItem(ACTIVE_SESSION_KEY, id)
  }

  static async clearActiveSession(): Promise<void> {
    localStorage.removeItem(ACTIVE_SESSION_KEY)
  }

  private static saveToStorage(sessions: Session[]): void {
    try {
      for (const session of sessions) {
        session.updatedAt = session.updatedAt || new Date().toISOString()
      }

      localStorage.setItem(SESSIONS_STORAGE_KEY, JSON.stringify(sessions))
    } catch (error) {
      console.error('Failed to save session:', error)
      throw error
    }
  }

  private static generateDefaultTitle(message: string): string {
    const maxLength = 50
    const cleaned = message.trim()

    if (cleaned.length <= maxLength) {
      return cleaned
    }

    return cleaned.substring(0, maxLength - 3) + '...'
  }

  static async clearAllSessions(): Promise<void> {
    localStorage.removeItem(SESSIONS_STORAGE_KEY)
    localStorage.removeItem(ACTIVE_SESSION_KEY)
  }
}
