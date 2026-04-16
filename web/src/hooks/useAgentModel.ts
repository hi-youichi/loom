import { useState, useEffect, useCallback } from 'react'

const STORAGE_KEY = 'loom-agent-model-map'
const LAST_MODEL_KEY = 'loom-last-selected-model'

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

function saveLastModel(model: string) {
  try {
    localStorage.setItem(LAST_MODEL_KEY, model)
  } catch {}
}

function loadLastModel(): string | null {
  try {
    return localStorage.getItem(LAST_MODEL_KEY)
  } catch {
    return null
  }
}

export function useAgentModel(agentId: string | null, models: { id: string }[]) {
  const [selectedModel, setSelectedModel] = useState(() => {
    // 1. Try agent-specific persisted model
    const map = loadMap()
    if (agentId && map[agentId]) {
      return map[agentId]
    }
    // 2. Try globally last-selected model
    const last = loadLastModel()
    if (last) {
      return last
    }
    return ''
  })

  useEffect(() => {
    // 1. Try agent-specific persisted model
    const map = loadMap()
    if (agentId && map[agentId]) {
      setSelectedModel(map[agentId])
      return
    }
    // 2. Try globally last-selected model
    const last = loadLastModel()
    if (last && models.some(m => m.id === last)) {
      setSelectedModel(last)
      return
    }
    // 3. Fallback: prefer known models, then first available
    if (models.length === 0) return
    const preferred = ['claude-3-5-sonnet', 'gpt-4o', 'gpt-4']
    for (const name of preferred) {
      const match = models.find(m => m.id.includes(name) || m.name.includes(name))
      if (match) {
        setSelectedModel(match.id)
        return
      }
    }
    setSelectedModel(models[0].id)
  }, [agentId, models])

  const handleModelChange = useCallback((model: string) => {
    setSelectedModel(model)
    // Save to agent-specific map
    if (agentId) {
      const map = loadMap()
      map[agentId] = model
      saveMap(map)
    }
    // Always save as globally last-selected
    saveLastModel(model)
  }, [agentId])

  return { selectedModel, handleModelChange }
}
