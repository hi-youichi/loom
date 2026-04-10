import { useState, useEffect, useCallback, useRef } from 'react'
import { getAvailableModels } from '../services/model'

const CACHE_KEY = 'loom-models-cache'
const CACHE_DURATION = 60 * 60 * 1000 // 1 hour

interface CachedModels {
  models: Model[]
  timestamp: number
}

export interface Model {
  id: string
  name: string
  provider: string
  family?: string
  capabilities?: string[]
}

function getCachedModels(): Model[] | null {
  try {
    const cached = localStorage.getItem(CACHE_KEY)
    if (!cached) return null
    
    const data: CachedModels = JSON.parse(cached)
    const isExpired = Date.now() - data.timestamp > CACHE_DURATION
    
    if (isExpired) {
      localStorage.removeItem(CACHE_KEY)
      return null
    }
    
    return data.models
  } catch {
    return null
  }
}

function setCachedModels(models: Model[]): void {
  try {
    const data: CachedModels = {
      models,
      timestamp: Date.now()
    }
    localStorage.setItem(CACHE_KEY, JSON.stringify(data))
  } catch {
    // Ignore cache errors
  }
}

export function useModels() {
  const [models, setModels] = useState<Model[]>(() => {
    return getCachedModels() || []
  })
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const mountedRef = useRef(true)

  const fetchModels = useCallback(async (forceRefresh = false) => {
    if (!forceRefresh) {
      const cached = getCachedModels()
      if (cached && cached.length > 0) {
        setModels(cached)
        return
      }
    }

    setLoading(true)
    setError(null)
    try {
      const data = await getAvailableModels()
      setCachedModels(data)
      if (mountedRef.current) {
        setModels(data)
      }
    } catch (e: unknown) {
      if (mountedRef.current) {
        const errorMessage = e instanceof Error ? e.message : 'Failed to load models'
        setError(errorMessage)
        console.error('Failed to load models:', e)
      }
    } finally {
      if (mountedRef.current) {
        setLoading(false)
      }
    }
  }, [])

  useEffect(() => {
    fetchModels()
    return () => {
      mountedRef.current = false
    }
  }, [fetchModels])

  return {
    models,
    loading,
    error,
    refetch: () => fetchModels(true),
  }
}