import { useState, useCallback, useEffect } from 'react'

import { ChatErrorBoundary } from '../components/error/ErrorBoundary'
import { FileTreeSidebar } from '../components/file-tree'
import { DashboardView } from '../components/dashboard'
import { AgentChatSidebar } from '../components/chat'
import { WorkspaceSelector } from '../components/workspace'
import { useWorkspace } from '../hooks/useWorkspace'
import { useThread } from '../hooks/useThread'
import { useAgents } from '../hooks/useAgents'
import { useChat } from '../hooks/useChat'
import { useChatPanel } from '../hooks/useChatPanel'
import type { FileNode } from '../components/file-tree'
import type { Session } from '../types/session'

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

export function ChatPage() {
  const {
    workspaces,
    activeWorkspaceId,
    threads,
    loading: workspaceLoading,
    error: workspaceError,
    loadWorkspaces,
    createWorkspace,
    selectWorkspace: selectWs,
  } = useWorkspace()
  const { agents } = useAgents({ autoRefresh: true, refreshInterval: 15000 })
  const { threadId } = useThread()
  const { selectedAgentId } = useChatPanel()
  const [selectedFileId, setSelectedFileId] = useState<string | null>(null)

  const {
    messages,
    isStreaming,
    sendMessage: sendRealMessage,
  } = useChat({
    threadId,
    agentId: selectedAgentId || 'dev',
  })

  useEffect(() => {
    loadWorkspaces()
  }, [loadWorkspaces])

  useEffect(() => {
    if (activeWorkspaceId) {
      selectWs(activeWorkspaceId)
    }
  }, [activeWorkspaceId, selectWs])

  const sessions: Session[] = threads.map(t => ({
    id: t.thread_id,
    title: t.thread_id.slice(0, 8),
    createdAt: new Date(t.created_at_ms).toISOString(),
    updatedAt: new Date(t.created_at_ms).toISOString(),
    lastMessage: '',
    messageCount: 0,
    agent: '',
    model: '',
    isPinned: false,
  }))

  const handleSelectWorkspace = useCallback((id: string) => {
    selectWs(id)
  }, [selectWs])

  const handleCreateWorkspace = useCallback(async (name?: string) => {
    return createWorkspace(name)
  }, [createWorkspace])

  const handleSendMessage = useCallback(async (text: string) => {
    await sendRealMessage(text)
  }, [sendRealMessage])

  return (
    <ChatErrorBoundary>
      <div className="flex h-screen overflow-hidden">
        <FileTreeSidebar
          files={DEMO_FILES}
          selectedId={selectedFileId}
          onSelect={(node) => setSelectedFileId(node.id)}
          workspaceSlot={
            <WorkspaceSelector
              workspaces={workspaces}
              activeWorkspaceId={activeWorkspaceId}
              loading={workspaceLoading}
              error={workspaceError}
              onSelect={handleSelectWorkspace}
              onCreate={handleCreateWorkspace}
              onRefresh={loadWorkspaces}
            />
          }
        />
        <div className="flex-1 min-w-0">
        <DashboardView
            agents={agents}
            activity={[]}
            activeCount={agents.filter(a => a.status === 'running').length}
            totalCalls={agents.reduce((sum, a) => sum + a.callCount, 0)}
            sessions={sessions}
          />
        </div>
        <AgentChatSidebar
          agents={agents.map(agent => ({
            name: agent.name,
            status: agent.status,
          }))}
          messages={messages}
          isStreaming={isStreaming}
          onSendMessage={handleSendMessage}
        />
      </div>
    </ChatErrorBoundary>
  )
}
