import { useState, useEffect, useCallback } from 'react'

const STORAGE_KEY = 'loom-agent-model-map'

type AgentModelMap = Record<string, string>

function loadMap(): AgentModelMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw ? JSON.parse(raw) : {}
  } catch {
    return {}
  }
}

function saveMap(map: AgentModelMap) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(map))
  } catch {}
}

export function useAgentModel(agentId: string | null, models: { id: string }[]) {
  const [selectedModel, setSelectedModel] = useState(() => {
    const map = loadMap()
    return (agentId && map[agentId]) || ''
  })

  useEffect(() => {
    const map = loadMap()
    if (agentId && map[agentId]) {
      setSelectedModel(map[agentId])
      return
    }
    if (models.length === 0) return
    const fallback = 'claude-3-5-sonnet'
    const match = models.find(m => m.id.includes(fallback))
    const resolved = match?.id || models[0].id
    setSelectedModel(resolved)
  }, [agentId, models])

  const handleModelChange = useCallback((model: string) => {
    setSelectedModel(model)
    if (agentId) {
      const map = loadMap()
      map[agentId] = model
      saveMap(map)
    }
  }, [agentId])

  return { selectedModel, handleModelChange }
}
