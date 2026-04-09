"use client"

import { memo } from "react"
import { useChatPanel } from "@/hooks/useChatPanel"
import { CollapsedPanel } from "./CollapsedPanel"
import { MessageList } from "./MessageList"
import { MessageComposer } from "../MessageComposer"
import type { UIMessageItemProps } from "@/types/ui/message"

interface AgentChatSidebarProps {
  agents: Array<{ name: string; status: string }>
  messages: UIMessageItemProps[]
  unreadCount: number
  onSendMessage: (text: string) => Promise<void>
}

export const AgentChatSidebar = memo(function AgentChatSidebar({
  agents,
  messages,
  unreadCount,
  onSendMessage,
}: AgentChatSidebarProps) {
  const { collapsed, width, selectedAgentId, toggle, expand, selectAgent } = useChatPanel()

  if (collapsed) {
    return (
      <CollapsedPanel
        unreadCount={unreadCount}
        onExpand={expand}
      />
    )
  }

  return (
    <div
      className="h-full border-l border-border bg-background flex flex-col shrink-0"
      style={{ width }}
    >
      {/* Header */}
      <div className="flex items-center justify-between p-3 border-b border-border">
        <div className="flex-1 min-w-0">
          <select
            value={selectedAgentId || ''}
            onChange={(e) => selectAgent(e.target.value)}
            className="w-full bg-transparent text-sm font-medium focus:outline-none cursor-pointer"
          >
            <option value="">选择 Agent</option>
            {agents.map((agent) => (
              <option key={agent.name} value={agent.name}>
                {agent.name}
              </option>
            ))}
          </select>
        </div>
        <button
          onClick={toggle}
          className="p-1 rounded hover:bg-accent/50 transition-colors text-muted-foreground ml-2 flex-shrink-0"
          aria-label="收起聊天面板"
        >
          ◀
        </button>
      </div>

      {/* Messages */}
      <div className="flex-1 overflow-y-auto">
        {messages.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center p-8 text-center">
            <div className="text-4xl mb-3">💬</div>
            <p className="text-sm text-muted-foreground mb-2">选择一个 Agent 开始对话</p>
            <p className="text-xs text-muted-foreground/60">Agent 将帮助您完成任务和回答问题</p>
          </div>
        ) : (
          <MessageList messages={messages} />
        )}
      </div>

      {/* Composer */}
      <div className="p-3 border-t border-border">
        <MessageComposer
          disabled={!selectedAgentId}
          onSend={onSendMessage}
        />
      </div>
    </div>
  )
})