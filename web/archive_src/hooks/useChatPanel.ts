import { useState, useEffect, useCallback } from "react"

interface ChatPanelState {
  collapsed: boolean
  width: number
  selectedAgentId: string | null
}

interface UseChatPanelReturn {
  // 状态
  collapsed: boolean
  width: number
  selectedAgentId: string | null
  
  // 操作
  toggle: () => void
  expand: () => void
  collapse: () => void
  setWidth: (width: number) => void
  selectAgent: (agentId: string) => void
  reset: () => void
}

const STORAGE_KEY = "chatPanelState"
const DEFAULT_STATE: ChatPanelState = {
  collapsed: false,
  width: 400,
  selectedAgentId: null,
}

const MIN_WIDTH = 320
const MAX_WIDTH = 600
const COLLAPSE_THRESHOLD = 200

export function useChatPanel(): UseChatPanelReturn {
  // 从 localStorage 初始化状态
  const [state, setState] = useState<ChatPanelState>(() => {
    try {
      const saved = localStorage.getItem(STORAGE_KEY)
      if (saved) {
        const parsed = JSON.parse(saved)
        // 验证保存的状态结构
        return {
          collapsed: typeof parsed.collapsed === "boolean" ? parsed.collapsed : DEFAULT_STATE.collapsed,
          width: typeof parsed.width === "number" && parsed.width >= MIN_WIDTH && parsed.width <= MAX_WIDTH ? parsed.width : DEFAULT_STATE.width,
          selectedAgentId: typeof parsed.selectedAgentId === "string" ? parsed.selectedAgentId : DEFAULT_STATE.selectedAgentId,
        }
      }
    } catch (e) {
      console.warn("Failed to load chat panel state:", e)
    }
    return DEFAULT_STATE
  })

  // 同步状态到 localStorage
  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(state))
    } catch (e) {
      console.warn("Failed to save chat panel state:", e)
    }
  }, [state])

  // 操作函数
  const toggle = useCallback(() => setState((prev) => ({
    ...prev,
    collapsed: !prev.collapsed,
  })), [])

  const expand = useCallback(() => setState((prev) => ({ ...prev, collapsed: false })), [])

  const collapse = useCallback(() => setState((prev) => ({ ...prev, collapsed: true })), [])

  // width is clamped to [MIN_WIDTH, MAX_WIDTH]; collapsed is set when the
  // *original* drag width drops below COLLAPSE_THRESHOLD (which is below MIN_WIDTH),
  // so this branch only fires when the user deliberately drags the panel narrow.
  const setWidth = useCallback((width: number) => {
    const clamped = Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, width))
    setState((prev) => ({ ...prev, width: clamped, collapsed: width < COLLAPSE_THRESHOLD }))
  }, [])

  const selectAgent = useCallback((agentId: string) => {
    setState((prev) => ({ ...prev, selectedAgentId: agentId }))
  }, [])

  const reset = useCallback(() => setState(DEFAULT_STATE), [])

  return {
    collapsed: state.collapsed,
    width: state.width,
    selectedAgentId: state.selectedAgentId,
    toggle,
    expand,
    collapse,
    setWidth,
    selectAgent,
    reset,
  }
}
