import type { Session } from '@/types/session'

// Mock session data for development and testing
export const mockSessions: Session[] = [
  {
    id: 'session-1',
    title: '代码重构讨论',
    createdAt: '2024-01-15T10:30:00Z',
    updatedAt: '2024-01-15T14:30:00Z',
    lastMessage: '我建议使用 TypeScript 重构整个项目，这样可以提高代码质量和开发效率。',
    messageCount: 5,
    agent: 'dev',
    model: 'anthropic/claude-3-5-sonnet-20241022',
    workspace: 'my-project',
    tags: ['重构', 'TypeScript'],
    isPinned: true,
    isArchived: false
  },
  {
    id: 'session-2',
    title: 'API 设计讨论',
    createdAt: '2024-01-14T09:00:00Z',
    updatedAt: '2024-01-14T16:00:00Z',
    lastMessage: '关于认证机制，我认为应该使用 JWT 而不是 session，这样可以更好地支持移动端。',
    messageCount: 12,
    agent: 'ask',
    model: 'openai/gpt-4',
    workspace: 'api-project',
    tags: ['API', 'JWT', '认证'],
    isPinned: false,
    isArchived: false
  },
  {
    id: 'session-3',
    title: '前端性能优化',
    createdAt: '2024-01-13T11:15:00Z',
    updatedAt: '2024-01-13T18:30:00Z',
    lastMessage: '滚动条样式需要优化，现在的样式在暗色模式下不够明显。',
    messageCount: 8,
    agent: 'dev',
    model: 'anthropic/claude-3-5-sonnet-20241022',
    workspace: 'web-app',
    tags: ['性能', 'CSS', '滚动条'],
    isPinned: false,
    isArchived: false
  },
  {
    id: 'session-4',
    title: '项目规划讨论',
    createdAt: '2024-01-12T08:00:00Z',
    updatedAt: '2024-01-12T17:00:00Z',
    lastMessage: '下个季度的技术路线图已经制定完成，主要包括微服务架构升级和 CI/CD 优化。',
    messageCount: 3,
    agent: 'dev',
    model: 'anthropic/claude-3-5-sonnet-20241022',
    workspace: 'company-project',
    tags: ['规划', '路线图'],
    isPinned: false,
    isArchived: false
  },
  {
    id: 'session-5',
    title: '数据库优化建议',
    createdAt: '2024-01-11T14:20:00Z',
    updatedAt: '2024-01-11T19:45:00Z',
    lastMessage: '建议在用户表上添加复合索引，可以显著提升查询性能。',
    messageCount: 6,
    agent: 'ask',
    model: 'openai/gpt-4',
    workspace: 'database',
    tags: ['数据库', '性能', '索引'],
    isPinned: true,
    isArchived: false
  },
  {
    id: 'session-6',
    title: '测试策略讨论',
    createdAt: '2024-01-10T10:00:00Z',
    updatedAt: '2024-01-10T16:30:00Z',
    lastMessage: '单元测试覆盖率应该达到 80% 以上，这是质量保证的基础。',
    messageCount: 4,
    agent: 'dev',
    model: 'anthropic/claude-3-5-sonnet-20241022',
    workspace: 'testing',
    tags: ['测试', '质量保证'],
    isPinned: false,
    isArchived: false
  }
]

// Helper function to get sessions for different scenarios
export function getSessionsForDemo(scenario?: 'empty' | 'few' | 'many'): Session[] {
  switch (scenario) {
    case 'empty':
      return []
    case 'few':
      return mockSessions.slice(0, 2)
    case 'many':
      return mockSessions
    default:
      return mockSessions
  }
}
