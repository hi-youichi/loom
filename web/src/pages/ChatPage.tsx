import { useState, useCallback, useEffect } from 'react'

import { ChatErrorBoundary } from '../components/error/ErrorBoundary'
import { FileTreeSidebar } from '../components/file-tree'
import { DashboardView } from '../components/dashboard'
import { AgentChatSidebar } from '../components/chat'
import { WorkspaceSelector } from '../components/workspace'
import { useWorkspace } from '../hooks/useWorkspace'
import { useSessionId } from '../hooks/useSessionId'
import { useAgents } from '../hooks/useAgents'
import { useChat } from '../hooks/useChat'
import { useChatPanel } from '../hooks/useChatPanel'
import { useModels } from '../hooks/useModels'
import { useRealtimeSessions } from '../hooks/useRealtimeSessions'
import type { FileNode } from '../components/file-tree'

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
    name: 'README.md',
    type: 'file',
    path: 'README.md',
    extension: 'md',
  },
]

export function ChatPage() {
  const {
    workspaces,
    activeWorkspaceId,
    loading: workspaceLoading,
    error: workspaceError,
    loadWorkspaces,
    createWorkspace,
    selectWorkspace: selectWs,
  } = useWorkspace()
  const { agents } = useAgents({ autoRefresh: true, refreshInterval: 15000 })
  const { sessionId, setSessionId, resetSession } = useSessionId(activeWorkspaceId ?? undefined)
  const { selectedAgentId } = useChatPanel()
  const [selectedFileId, setSelectedFileId] = useState<string | null>(null)
  const { models } = useModels()
  const [selectedModel, setSelectedModel] = useState('')

  useEffect(() => {
    if (selectedModel || models.length === 0) return
    const fallback = 'claude-3-5-sonnet'
    const match = models.find(m => m.id.includes(fallback) || m.name.includes(fallback))
    setSelectedModel(match?.id || models[0].id)
  }, [models, selectedModel])

  const {
    messages,
    isStreaming,
    sendMessage: sendRealMessage,
    cancel,
    loadHistory,
  } = useChat({
    sessionId,
    workspaceId: activeWorkspaceId ?? undefined,
    agentId: selectedAgentId || 'dev',
    model: selectedModel,
  })

  useEffect(() => {
    loadWorkspaces()
  }, [loadWorkspaces])

  useEffect(() => {
    if (activeWorkspaceId) {
      selectWs(activeWorkspaceId)
    }
  }, [activeWorkspaceId, selectWs])

  // Use real-time sessions hook for automatic updates via WebSocket
  const { sessions, loading: loadingSessions } = useRealtimeSessions(activeWorkspaceId ?? undefined)

  const handleSelectWorkspace = useCallback((id: string) => {
    selectWs(id)
  }, [selectWs])

  const handleCreateWorkspace = useCallback(async (name?: string) => {
    return createWorkspace(name)
  }, [createWorkspace])

  const handleSendMessage = useCallback(async (text: string) => {
    await sendRealMessage(text)
  }, [sendRealMessage])

  const handleSelectSession = useCallback(async (targetSessionId: string) => {
    setSessionId(targetSessionId)
    if (loadHistory) {
      await loadHistory(targetSessionId)
    }
  }, [loadHistory, setSessionId])

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
            loadingSessions={loadingSessions}
            onSelectSession={handleSelectSession}
            onNewSession={resetSession}
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
          onCancel={cancel}
          onModelChange={setSelectedModel}
        />
      </div>
    </ChatErrorBoundary>
  )
}
