/**
 * Session service for managing chat sessions
 */

import type { Session } from '../types/session'
import { getConnection } from './connection'

const SESSIONS_STORAGE_KEY = 'loom-sessions'
const ACTIVE_SESSION_KEY = 'loom-active-session'

/**
 * Session service class
 */
export class SessionService {
  /**
   * Get all sessions from local storage
   */
  static async listSessions(): Promise<Session[]> {
    try {
      const stored = localStorage.getItem(SESSIONS_STORAGE_KEY)
      if (!stored) return []
      
      const sessions: Session[] = JSON.parse(stored)
      // Filter out deleted sessions
      return sessions.filter(s => s.status !== 'deleted')
    } catch (error) {
      console.error('Failed to load sessions:', error)
      return []
    }
  }

  /**
   * Get a specific session by ID
   */
  static async getSession(id: string): Promise<Session | null> {
    const sessions = await this.listSessions()
    return sessions.find(s => s.id === id) || null
  }

  /**
   * Create a new session
   */
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
      model: data.model || '',
      workspace: data.workspace,
      tags: data.tags || [],
      isPinned: false,
      isArchived: false,
      status: 'active',
    }

    await this.saveSession(newSession)
    return newSession
  }

  /**
   * Update an existing session
   */
  static async updateSession(id: string, data: Partial<Session>): Promise<Session | null> {
    const session = await this.getSession(id)
    if (!session) return null

    const updatedSession: Session = {
      ...session,
      ...data,
      id: session.id, // Ensure ID doesn't change
      updatedAt: new Date().toISOString(),
    }

    await this.saveSession(updatedSession)
    return updatedSession
  }

  /**
   * Delete a session (soft delete)
   */
  static async deleteSession(id: string): Promise<boolean> {
    const session = await this.getSession(id)
    if (!session) return false

    session.status = 'deleted'
    await this.saveSession(session)
    return true
  }

  /**
   * Permanently delete a session
   */
  static async permanentlyDeleteSession(id: string): Promise<boolean> {
    const sessions = await this.listSessions()
    const filtered = sessions.filter(s => s.id !== id)
    
    try {
      localStorage.setItem(SESSIONS_STORAGE_KEY, JSON.stringify(filtered))
      return true
    } catch (error) {
      console.error('Failed to permanently delete session:', error)
      return false
    }
  }

  /**
   * Pin or unpin a session
   */
  static async togglePinSession(id: string): Promise<Session | null> {
    const session = await this.getSession(id)
    if (!session) return null

    return this.updateSession(id, { isPinned: !session.isPinned })
  }

  /**
   * Archive or unarchive a session
   */
  static async toggleArchiveSession(id: string): Promise<Session | null> {
    const session = await this.getSession(id)
    if (!session) return null

    return this.updateSession(id, { isArchived: !session.isArchived })
  }

  /**
   * Add a message to a session
   */
  static async addMessage(
    sessionId: string,
    message: string,
    sender: 'user' | 'assistant'
  ): Promise<Session | null> {
    const session = await this.getSession(sessionId)
    if (!session) return null

    return this.updateSession(sessionId, {
      lastMessage: message.substring(0, 200), // Keep preview short
      messageCount: session.messageCount + 1,
      updatedAt: new Date().toISOString(),
    })
  }

  /**
   * Get or create active session
   */
  static async getActiveSession(): Promise<Session> {
    try {
      const activeId = localStorage.getItem(ACTIVE_SESSION_KEY)
      if (activeId) {
        const session = await this.getSession(activeId)
        if (session && session.status === 'active') {
          return session
        }
      }
    } catch (error) {
      console.error('Failed to get active session:', error)
    }

    // Create new session if no active session exists
    const newSession = await this.createSession()
    localStorage.setItem(ACTIVE_SESSION_KEY, newSession.id)
    return newSession
  }

  /**
   * Set active session
   */
  static async setActiveSession(id: string): Promise<void> {
    localStorage.setItem(ACTIVE_SESSION_KEY, id)
  }

  /**
   * Export session as markdown
   */
  static async exportSessionAsMarkdown(id: string): Promise<string | null> {
    const session = await this.getSession(id)
    if (!session) return null

    const lines = [
      `# ${session.title}`,
      '',
      `**Agent:** ${session.agent}`,
      `**Model:** ${session.model}`,
      `**Created:** ${new Date(session.createdAt).toLocaleString()}`,
      `**Updated:** ${new Date(session.updatedAt).toLocaleString()}`,
      '',
      '---',
      '',
      `*Messages: ${session.messageCount}*`,
      `*Last message:* ${session.lastMessage}`,
      '',
      '*Note: Full message content would be stored separately.*',
    ]

    return lines.join('\n')
  }

  /**
   * Export session as JSON
   */
  static async exportSessionAsJSON(id: string): Promise<string | null> {
    const session = await this.getSession(id)
    if (!session) return null

    return JSON.stringify(session, null, 2)
  }

  /**
   * Search sessions
   */
  static async searchSessions(query: string): Promise<Session[]> {
    const sessions = await this.listSessions()
    const lowerQuery = query.toLowerCase()

    return sessions.filter(session =>
      session.title.toLowerCase().includes(lowerQuery) ||
      session.lastMessage.toLowerCase().includes(lowerQuery) ||
      session.tags?.some(tag => tag.toLowerCase().includes(lowerQuery))
    )
  }

  /**
   * Save a single session to storage
   */
  private static async saveSession(session: Session): Promise<void> {
    try {
      const sessions = await this.listSessions()
      const index = sessions.findIndex(s => s.id === session.id)
      
      if (index >= 0) {
        sessions[index] = session
      } else {
        sessions.push(session)
      }

      localStorage.setItem(SESSIONS_STORAGE_KEY, JSON.stringify(sessions))
    } catch (error) {
      console.error('Failed to save session:', error)
      throw error
    }
  }

  /**
   * Generate a default title from the first message
   */
  private static generateDefaultTitle(message: string): string {
    const maxLength = 50
    const cleaned = message.trim()
    
    if (cleaned.length <= maxLength) {
      return cleaned
    }

    return cleaned.substring(0, maxLength - 3) + '...'
  }

  /**
   * Clear all sessions (for testing purposes)
   */
  static async clearAllSessions(): Promise<void> {
    localStorage.removeItem(SESSIONS_STORAGE_KEY)
    localStorage.removeItem(ACTIVE_SESSION_KEY)
  }
}
