/**
 * Utility functions for session formatting and display
 */

import type { Session } from '../types/session'

/**
 * Format relative time (e.g., "2 hours ago", "3 days ago")
 */
export function formatRelativeTime(dateString: string): string {
  const date = new Date(dateString)
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffSecs = Math.floor(diffMs / 1000)
  const diffMins = Math.floor(diffSecs / 60)
  const diffHours = Math.floor(diffMins / 60)
  const diffDays = Math.floor(diffHours / 24)

  if (diffSecs < 60) return '刚刚'
  if (diffMins < 60) return `${diffMins} 分钟前`
  if (diffHours < 24) return `${diffHours} 小时前`
  if (diffDays < 7) return `${diffDays} 天前`
  
  // For older dates, show actual date
  return date.toLocaleDateString('zh-CN', {
    month: 'short',
    day: 'numeric',
  })
}

/**
 * Format full date and time
 */
export function formatDateTime(dateString: string): string {
  const date = new Date(dateString)
  return date.toLocaleString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

/**
 * Format session preview text
 */
export function formatPreviewText(text: string, maxLength: number = 100): string {
  if (text.length <= maxLength) return text
  return text.substring(0, maxLength - 3) + '...'
}

/**
 * Get session display name (fallback to untitled)
 */
export function getSessionDisplayName(session: Session): string {
  return session.title.trim() || '未命名对话'
}

/**
 * Get agent display name
 */
export function getAgentDisplayName(agent: string): string {
  const agentNames: Record<string, string> = {
    'dev': '开发助手',
    'ask': '问答助手',
    'assistant': '通用助手',
    'explore': '探索助手',
  }
  return agentNames[agent] || agent
}

/**
 * Get model display name (short version)
 */
export function getModelDisplayName(model: string): string {
  if (!model) return '未知模型'
  
  // Extract just the model name from provider/model format
  const parts = model.split('/')
  const modelName = parts[parts.length - 1]
  
  // Shorten common model names
  if (modelName.includes('claude-3-5-sonnet')) return 'Claude 3.5 Sonnet'
  if (modelName.includes('gpt-4')) return 'GPT-4'
  if (modelName.includes('gpt-3.5')) return 'GPT-3.5'
  
  return modelName
}

/**
 * Get session status badge style
 */
export function getSessionStatusStyle(status: string): string {
  switch (status) {
    case 'active':
      return 'bg-green-500/10 text-green-600'
    case 'archived':
      return 'bg-gray-500/10 text-gray-600'
    case 'deleted':
      return 'bg-red-500/10 text-red-600'
    default:
      return 'bg-gray-500/10 text-gray-600'
  }
}

/**
 * Truncate text with ellipsis
 */
export function truncateText(text: string, length: number): string {
  if (text.length <= length) return text
  return text.substring(0, length) + '...'
}
