"use client"

import { memo, useRef, useCallback, useEffect } from "react"
import { ChevronRight, Users, ChevronDown } from "lucide-react"
import { useChatPanel } from "@/hooks/useChatPanel"
import { MessageList } from "./MessageList"
import { MessageComposer } from "../MessageComposer"
import { ThemeToggle } from "../ThemeToggle"
import { useModels } from "@/hooks/useModels"
import { useAgentModel } from "@/hooks/useAgentModel"
import type { UIMessageItemProps } from "@/types/ui/message"

interface AgentChatSidebarProps {
  agents: Array<{ name: string; status: string }>
  messages: UIMessageItemProps[]
  isStreaming?: boolean
  onSendMessage: (text: string) => Promise<void>
  onCancel?: () => void
  onModelChange?: (model: string) => void
}

function ResizeHandle({ onDrag, onToggle }: { onDrag: (w: number) => void; onToggle: () => void }) {
  const dragging = useRef(false)
  const startX = useRef(0)
  const isDragging = useRef(false)

  const onPointerDown = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault()
    dragging.current = true
    isDragging.current = false
    startX.current = e.clientX
    document.body.style.userSelect = "none"
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
  }, [])

  const onPointerMove = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging.current) return
    isDragging.current = true
    const deltaX = e.clientX - startX.current
    const newWidth = (e.currentTarget.parentElement as HTMLElement).offsetWidth - deltaX
    onDrag(newWidth)
  }, [onDrag])

  const onPointerUp = useCallback(() => {
    dragging.current = false
    document.body.style.userSelect = ""
  }, [])

  const handlePointerUp = useCallback((_e: React.PointerEvent<HTMLDivElement>) => {
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
  isStreaming = false,
  onSendMessage,
  onCancel,
  onModelChange,
}: AgentChatSidebarProps) {
  const { collapsed, width, selectedAgentId, toggle, expand, setWidth, selectAgent } = useChatPanel()
  const { models } = useModels()
  const { selectedModel, handleModelChange: setModel } = useAgentModel(selectedAgentId, models)

  const selectedAgentName = agents.find((a) => a.name === selectedAgentId)?.name || selectedAgentId

  useEffect(() => {
    if (agents.length > 0 && !selectedAgentId) {
      selectAgent(agents[0].name)
    }
  }, [agents, selectedAgentId, selectAgent])

  const handleModelChange = (model: string) => {
    setModel(model)
    onModelChange?.(model)
  }

  return (
    <aside
      className="relative h-full bg-muted/30 border-l border-border flex flex-col"
      style={{ width: collapsed ? 0 : width, minWidth: collapsed ? 0 : 320 }}
    >
      <ResizeHandle
        onDrag={(w) => setWidth(w)}
        onToggle={toggle}
      />

      <button
        onClick={expand}
        className="absolute right-0 top-1/2 -translate-y-1/2 p-1 rounded-l-md bg-muted-foreground/10 hover:bg-muted-foreground/20 transition-colors"
        style={{ display: collapsed ? 'block' : 'none' }}
        aria-label="展开侧边栏"
      >
        <ChevronRight className="w-4 h-4" />
      </button>

      <div className="p-3 border-b border-border flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Users className="w-4 h-4 text-muted-foreground" />
          <div className="relative">
            <button
              onClick={() => {}}
              className="flex items-center gap-2 text-sm font-medium hover:text-primary transition-colors"
            >
              <span className="truncate">{selectedAgentName || '选择 Agent'}</span>
              <ChevronDown className="w-3 h-3" />
            </button>
            {/* Agent dropdown placeholder - would need full implementation */}
          </div>
        </div>
        <ThemeToggle />
      </div>

      <div className="flex-1 min-h-0 overflow-hidden">
        {collapsed ? null : (
          <MessageList messages={messages} streaming={isStreaming} />
        )}
      </div>

      <div className="border-t border-border">
        <MessageComposer
          disabled={!selectedAgentId || isStreaming}
          isStreaming={isStreaming}
          onSend={onSendMessage}
          onCancel={onCancel}
          selectedModel={selectedModel}
          onModelChange={handleModelChange}
        />
      </div>
    </aside>
  )
})
