import { useState, useCallback, useEffect } from 'react'

import { ChatErrorBoundary, FileTreeSidebar, DashboardView, AgentChatSidebar, WorkspaceSelector, ToastProvider } from '@loom/ui'
import { useWorkspace, useSessionId, useAgents, useChat, useChatPanel, useModels, useRealtimeSessions, useWorkspaceFiles, useAgentModel } from '@loom/hooks'
import { readFile as wsReadFile } from '@loom/service-workspace'
import type { FileNode as UIFileNode } from '@loom/ui'
import { TabBar } from '@loom/ui'

// -----------------------------------------------------------------------------
// Tab types
// -----------------------------------------------------------------------------

type Tab = {
  id: string
  title: string
  type: 'dashboard' | 'file'
  path?: string
}

// -----------------------------------------------------------------------------
// ChatPage
// -----------------------------------------------------------------------------

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
  const { models } = useModels()
  const { selectedModel, handleModelChange } = useAgentModel(selectedAgentId, models)

  const [tabs, setTabs] = useState<Tab[]>([
    { id: 'dashboard', title: '仪表盘', type: 'dashboard' },
  ])
  const [activeTabId, setActiveTabId] = useState('dashboard')

  const { rootFiles, loading: filesLoading, loadChildren, refresh: refreshFiles } = useWorkspaceFiles(activeWorkspaceId)

  const {
    messages,
    isStreaming,
    sendMessage: sendRealMessage,
    cancel,
    loadHistory,
    error: chatError,
    dismissError,
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

  // Handle file selection from tree → open tab
  const handleFileSelect = useCallback((node: UIFileNode) => {
    if (node.type === 'folder') {
      // Expand folder via loadChildren
      loadChildren(node.path)
      return
    }

    // Open file in new tab (or focus existing)
    const tabId = `file:${node.path}`
    setTabs(prev => {
      if (prev.some(t => t.id === tabId)) return prev
      return [...prev, { id: tabId, title: node.name, type: 'file', path: node.path }]
    })
    setActiveTabId(tabId)
  }, [loadChildren])

  const handleCloseTab = useCallback((tabId: string) => {
    setTabs(prev => {
      const next = prev.filter(t => t.id !== tabId)
      // If closing active tab, switch to dashboard
      return next
    })
    if (activeTabId === tabId) {
      setActiveTabId('dashboard')
    }
  }, [activeTabId])

  // Convert hook FileNode to UI FileNode (compatible types)
  const uiFiles: UIFileNode[] = rootFiles.map(f => convertNode(f))

  const activeTab = tabs.find(t => t.id === activeTabId) ?? tabs[0]

  return (
    <ToastProvider>
    <ChatErrorBoundary>
      <div className="flex h-screen overflow-hidden">
        <FileTreeSidebar
          files={uiFiles}
          selectedId={activeTab?.type === 'file' && activeTab.path ? `file:${activeTab.path}` : null}
          onSelect={handleFileSelect}
          loading={filesLoading}
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
          onRefresh={refreshFiles}
        />
        <div className="flex-1 min-w-0 flex flex-col">
          {/* Tab bar */}
          {tabs.length > 1 && (
            <TabBar
              tabs={tabs.map(t => ({
                id: t.id,
                title: t.title,
                closable: t.type !== 'dashboard',
              }))}
              activeId={activeTabId}
              onSelect={setActiveTabId}
              onClose={handleCloseTab}
            />
          )}
          {/* Tab content */}
          <div className="flex-1 min-h-0 overflow-auto">
            {activeTab?.type === 'dashboard' ? (
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
            ) : (
              <FileContentView filePath={activeTab?.path ?? ''} workspaceId={activeWorkspaceId ?? ''} />
            )}
          </div>
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
          onModelChange={handleModelChange}
          error={chatError}
          onDismissError={dismissError}
        />
      </div>
    </ChatErrorBoundary>
    </ToastProvider>
  )
}

// -----------------------------------------------------------------------------
// File content viewer (placeholder — reads file content from API)
// -----------------------------------------------------------------------------

function FileContentView({ filePath, workspaceId }: { filePath: string; workspaceId: string }) {
  const [content, setContent] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!filePath || !workspaceId) return
    setLoading(true)
    setError(null)
    wsReadFile(workspaceId, filePath)
      .then(c => { setContent(c); setLoading(false) })
      .catch(e => { setError(e instanceof Error ? e.message : 'Failed to read file'); setLoading(false) })
  }, [filePath, workspaceId])

  if (loading) {
    return <div className="flex items-center justify-center h-full text-muted-foreground text-sm">加载中...</div>
  }
  if (error) {
    return <div className="flex items-center justify-center h-full text-destructive text-sm">{error}</div>
  }
  return (
    <div className="p-4">
      <div className="text-xs text-muted-foreground mb-2">{filePath}</div>
      <pre className="text-sm whitespace-pre-wrap font-mono bg-muted/30 rounded p-4 overflow-auto max-h-[calc(100vh-6rem)]">
        {content}
      </pre>
    </div>
  )
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

function convertNode(node: import('@loom/hooks').FileNode): UIFileNode {
  return {
    id: node.id,
    name: node.name,
    type: node.type,
    path: node.path,
    extension: node.extension,
    size: node.size,
    children: node.children?.map(convertNode),
  }
}
