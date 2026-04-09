import { useState } from 'react'

import { ChatErrorBoundary } from '../components/error/ErrorBoundary'
import { FileTreeSidebar } from '../components/file-tree'
import { DashboardView } from '../components/dashboard'
import type { FileNode } from '../components/file-tree'
import type { AgentInfo, ActivityEvent } from '../types/agent'

const DEMO_FILES: FileNode[] = [
  {
    id: '1',
    name: 'src',
    type: 'folder',
    path: 'src',
    children: [
      {
        id: '1-1',
        name: 'App.tsx',
        type: 'file',
        path: 'src/App.tsx',
        extension: 'tsx',
      },
      {
        id: '1-2',
        name: 'main.tsx',
        type: 'file',
        path: 'src/main.tsx',
        extension: 'tsx',
      },
      {
        id: '1-3',
        name: 'index.css',
        type: 'file',
        path: 'src/index.css',
        extension: 'css',
      },
      {
        id: '1-4',
        name: 'components',
        type: 'folder',
        path: 'src/components',
        children: [
          {
            id: '1-4-1',
            name: 'MessageComposer.tsx',
            type: 'file',
            path: 'src/components/MessageComposer.tsx',
            extension: 'tsx',
          },
          {
            id: '1-4-2',
            name: 'ThinkIndicator.tsx',
            type: 'file',
            path: 'src/components/ThinkIndicator.tsx',
            extension: 'tsx',
          },
        ],
      },
      {
        id: '1-5',
        name: 'hooks',
        type: 'folder',
        path: 'src/hooks',
        children: [
          {
            id: '1-5-1',
            name: 'useChat.ts',
            type: 'file',
            path: 'src/hooks/useChat.ts',
            extension: 'ts',
          },
        ],
      },
    ],
  },
  {
    id: '2',
    name: 'package.json',
    type: 'file',
    path: 'package.json',
    extension: 'json',
  },
  {
    id: '3',
    name: 'vite.config.ts',
    type: 'file',
    path: 'vite.config.ts',
    extension: 'ts',
  },
]

const now = Date.now()
const s = (ms: number) => new Date(now - ms).toISOString()

const DEMO_AGENTS: AgentInfo[] = [
  {
    name: 'dev',
    status: 'running',
    callCount: 12,
    lastRunAt: s(2000),
    lastError: null,
    profile: {
      name: 'dev',
      description: '默认开发 Agent，负责代码编写与调试',

      tools: ['bash', 'read', 'edit', 'write_file', 'glob', 'grep', 'websearch'],
      mcpServers: [],
      source: 'builtin',
    },
  },
  {
    name: 'reviewer',
    status: 'idle',
    callCount: 3,
    lastRunAt: s(300_000),
    lastError: null,
    profile: {
      name: 'reviewer',
      description: '代码审查 Agent',

      tools: ['read', 'glob', 'grep'],
      mcpServers: [],
      source: 'project',
    },
  },
  {
    name: 'researcher',
    status: 'running',
    callCount: 7,
    lastRunAt: s(1000),
    lastError: null,
    profile: {
      name: 'researcher',
      description: '搜索与研究 Agent',

      tools: ['websearch', 'web_fetcher', 'read', 'grep'],
      mcpServers: ['exa-search'],
      source: 'user',
    },
  },
  {
    name: 'linter',
    status: 'idle',
    callCount: 2,
    lastRunAt: s(180_000),
    lastError: null,
    profile: {
      name: 'linter',
      description: '代码检查 Agent',

      tools: ['bash', 'read', 'glob'],
      mcpServers: [],
      source: 'project',
    },
  },
  {
    name: 'deploy',
    status: 'idle',
    callCount: 5,
    lastRunAt: s(600_000),
    lastError: null,
    profile: {
      name: 'deploy',
      description: '部署与运维 Agent',

      tools: ['bash', 'read', 'write_file'],
      mcpServers: ['k8s-api'],
      source: 'user',
    },
  },
  {
    name: 'planner',
    status: 'idle',
    callCount: 1,
    lastRunAt: s(900_000),
    lastError: null,
    profile: {
      name: 'planner',
      description: '任务规划 Agent',

      tools: ['read', 'grep', 'glob'],
      mcpServers: [],
      source: 'builtin',
    },
  },
  {
    name: 'architect',
    status: 'idle',
    callCount: 4,
    lastRunAt: s(1_200_000),
    lastError: null,
    profile: {
      name: 'architect',
      description: '架构设计 Agent',

      tools: ['read', 'glob', 'grep', 'websearch'],
      mcpServers: ['figma-api'],
      source: 'user',
    },
  },
  {
    name: 'tester',
    status: 'idle',
    callCount: 6,
    lastRunAt: s(240_000),
    lastError: null,
    profile: {
      name: 'tester',
      description: '测试 Agent',

      tools: ['bash', 'read', 'write_file', 'glob'],
      mcpServers: [],
      source: 'project',
    },
  },
]

function makeActivity(): ActivityEvent[] {
  const events: ActivityEvent[] = [
    { id: 'a1', timestamp: s(2000), agent: 'dev', type: 'run_start', summary: '修复登录页面的表单验证问题', isError: false },
    { id: 'a2', timestamp: s(1900), agent: 'dev', type: 'tool_call', summary: 'read', isError: false },
    { id: 'a3', timestamp: s(1800), agent: 'dev', type: 'tool_start', summary: 'read', isError: false },
    { id: 'a4', timestamp: s(1700), agent: 'dev', type: 'tool_output', summary: '读取 src/components/LoginForm.tsx', isError: false },
    { id: 'a5', timestamp: s(1600), agent: 'researcher', type: 'run_start', summary: '调研 React 19 新特性', isError: false },
    { id: 'a6', timestamp: s(1500), agent: 'researcher', type: 'tool_call', summary: 'websearch', isError: false },
    { id: 'a7', timestamp: s(1400), agent: 'researcher', type: 'tool_start', summary: 'websearch', isError: false },
    { id: 'a8', timestamp: s(1300), agent: 'researcher', type: 'tool_output', summary: 'React 19 正式发布，新增 use() hook...', isError: false },
    { id: 'a9', timestamp: s(1200), agent: 'dev', type: 'tool_call', summary: 'edit', isError: false },
    { id: 'a10', timestamp: s(1100), agent: 'dev', type: 'tool_end', summary: 'edit done', isError: false },
    { id: 'a11', timestamp: s(1000), agent: 'dev', type: 'message_chunk', summary: '我已经修复了表单验证的逻辑，现在邮箱字段会正确校验格式。', isError: false },
    { id: 'a12', timestamp: s(5000), agent: 'linter', type: 'run_start', summary: '检查代码质量', isError: false },
    { id: 'a13', timestamp: s(4900), agent: 'linter', type: 'tool_call', summary: 'bash', isError: false },
    { id: 'a14', timestamp: s(4800), agent: 'linter', type: 'tool_end', summary: 'bash done', isError: false },
    { id: 'a15', timestamp: s(4700), agent: 'linter', type: 'message_chunk', summary: 'TypeError: Cannot read property "fix" of undefined', isError: false },
    { id: 'a16', timestamp: s(300_000), agent: 'reviewer', type: 'run_start', summary: '审查 PR #42 的代码变更', isError: false },
    { id: 'a17', timestamp: s(299_000), agent: 'reviewer', type: 'tool_call', summary: 'read', isError: false },
    { id: 'a18', timestamp: s(298_000), agent: 'reviewer', type: 'tool_end', summary: 'read done', isError: false },
    { id: 'a19', timestamp: s(297_000), agent: 'reviewer', type: 'message_chunk', summary: 'LGTM，只有两个小的命名建议。', isError: false },
    { id: 'a20', timestamp: s(240_000), agent: 'tester', type: 'run_start', summary: '运行集成测试', isError: false },
    { id: 'a21', timestamp: s(239_000), agent: 'tester', type: 'tool_call', summary: 'bash', isError: false },
    { id: 'a22', timestamp: s(238_000), agent: 'tester', type: 'tool_end', summary: 'bash done', isError: false },
    { id: 'a23', timestamp: s(600_000), agent: 'deploy', type: 'run_start', summary: '部署到 staging 环境', isError: false },
    { id: 'a24', timestamp: s(599_000), agent: 'deploy', type: 'tool_call', summary: 'bash', isError: false },
    { id: 'a25', timestamp: s(598_000), agent: 'deploy', type: 'tool_end', summary: 'bash done', isError: false },
    { id: 'a26', timestamp: s(900_000), agent: 'planner', type: 'run_start', summary: '规划 Sprint 12 任务', isError: false },
    { id: 'a27', timestamp: s(1_200_000), agent: 'architect', type: 'run_start', summary: '设计微服务拆分方案', isError: false },
  ]
  return events
}

const DEMO_ACTIVITY = makeActivity()
const DEMO_ACTIVE_COUNT = DEMO_AGENTS.filter((a) => a.status === 'running').length
const DEMO_TOTAL_CALLS = DEMO_AGENTS.reduce((sum, a) => sum + a.callCount, 0)

export function ChatPage() {
  const [selectedFileId, setSelectedFileId] = useState<string | null>(null)

  return (
    <ChatErrorBoundary>
      <div className="flex h-screen">
        <FileTreeSidebar
          files={DEMO_FILES}
          selectedId={selectedFileId}
          onSelect={(node) => setSelectedFileId(node.id)}
        />
        <div className="flex-1 min-w-0">
          <DashboardView
            agents={DEMO_AGENTS}
            activity={DEMO_ACTIVITY}
            activeCount={DEMO_ACTIVE_COUNT}
            totalCalls={DEMO_TOTAL_CALLS}
          />
        </div>
      </div>
    </ChatErrorBoundary>
  )
}
