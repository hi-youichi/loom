"use client"

import { memo, useRef, useCallback, useState, useEffect } from "react"
import { MessageSquare, ChevronRight, Users, ChevronDown } from "lucide-react"
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

function ResizeHandle({ onDrag, onToggle }: { onDrag: (w: number) => void; onToggle: () => void }) {
  const dragging = useRef(false)
  const startX = useRef(0)
  const startWidth = useRef(0)
  const isDragging = useRef(false)

  const onPointerDown = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault()
    dragging.current = true
    isDragging.current = false
    startX.current = e.clientX
    startWidth.current = (e.currentTarget.parentElement as HTMLElement).offsetWidth
    document.body.style.userSelect = "none"
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
  }, [])

  const onPointerMove = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging.current) return
    isDragging.current = true
    const deltaX = e.clientX - startX.current
    const newWidth = startWidth.current - deltaX
    onDrag(newWidth)
  }, [onDrag])

  const onPointerUp = useCallback(() => {
    dragging.current = false
    document.body.style.userSelect = ""
  }, [])

  const handlePointerUp = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    onPointerUp()
    if (!isDragging.current) {
      onToggle()
    }
  }, [onPointerUp, onToggle])

  return (
    <div
      className="absolute left-0 top-0 h-full w-1 cursor-col-resize hover:bg-primary/20 hover:w-1.5 -ml-0.5 transition-colors"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={handlePointerUp}
      onPointerLeave={onPointerUp}
    />
  )
}

export const AgentChatSidebar = memo(function AgentChatSidebar({
  agents,
  messages,
  unreadCount,
  onSendMessage,
}: AgentChatSidebarProps) {
  const { collapsed, width, selectedAgentId, toggle, expand, selectAgent, setWidth } = useChatPanel()
  const [popupOpen, setPopupOpen] = useState(false)
  const popupRef = useRef<HTMLDivElement>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    const onOutside = (e: MouseEvent) => {
      if (popupRef.current && !popupRef.current.contains(e.target as Node) && triggerRef.current && !triggerRef.current.contains(e.target as Node)) {
        setPopupOpen(false)
      }
    }
    document.addEventListener('mousedown', onOutside)
    return () => document.removeEventListener('mousedown', onOutside)
  }, [])

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
      className="relative h-full border-l border-border bg-background flex flex-col shrink-0"
      style={{ width }}
    >
      <ResizeHandle onDrag={setWidth} onToggle={toggle} />
      {/* Header */}
      <div className="flex items-center justify-between p-3 border-b border-border">
        <div className="flex items-center gap-1 flex-1 min-w-0">
          <button
            ref={triggerRef}
            onClick={() => setPopupOpen((v) => !v)}
            className="flex items-center gap-1.5 text-sm font-medium focus:outline-none cursor-pointer rounded px-1.5 py-1 hover:bg-accent/50 transition-colors"
            aria-label="切换 Agent"
          >
            <Users className="h-4 w-4 flex-shrink-0" />
            <span className="truncate">{selectedAgentId || 'Assistant'}</span>
            <span className="ml-auto text-muted-foreground"><ChevronDown className="h-3 w-3" /></span>
          </button>
        </div>
        <button
          onClick={toggle}
          className="p-1 rounded hover:bg-accent/50 transition-colors text-muted-foreground ml-2 flex-shrink-0"
          aria-label={collapsed ? "展开聊天面板" : "收起聊天面板"}
        >
          <ChevronRight className="h-4 w-4" />
        </button>

        {popupOpen && (
          <div
            ref={popupRef}
            className="absolute top-12 left-4 z-50 w-56 rounded-md border border-border bg-popover p-1 shadow-lg"
            role="listbox"
            aria-label="Agent 列表"
          >
            <button
              onClick={() => { selectAgent(''); setPopupOpen(false) }}
              className="w-full text-left px-3 py-1.5 text-sm rounded hover:bg-accent"
              role="option"
            >
              Assistant
            </button>
            {agents.map((agent) => (
              <button
                key={agent.name}
                onClick={() => { selectAgent(agent.name); setPopupOpen(false) }}
                className="w-full text-left px-3 py-1.5 text-sm rounded hover:bg-accent"
                role="option"
              >
                {agent.name}
              </button>
            ))}
          </div>
        )}
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