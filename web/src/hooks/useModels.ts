import { useState, useEffect, useCallback, useRef } from 'react'
import { getConnection, type Model } from '../services/connection'

const CACHE_KEY = 'loom-models-cache'
const CACHE_DURATION = 60 * 60 * 1000

interface CachedModels {
  models: Model[]
  timestamp: number
}

export type { Model }

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
  }
}

export function useModels() {
  const [models, setModels] = useState<Model[]>(() => {
    return getCachedModels() || []
  })
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const mountedRef = useRef(true)

  const refetch = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const id = crypto.randomUUID()
      const conn = getConnection()
      const response = await conn.request({ type: 'list_models', id }) as { models: Model[] }
      const data = response.models || []
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
    const handler = (data: Model[]) => {
      if (!mountedRef.current) return
      setCachedModels(data)
      setModels(data)
      setLoading(false)
      setError(null)
    }

    const conn = getConnection()
    conn.on('models_updated', handler)

    if (getCachedModels() === null) {
      refetch()
    }

    return () => {
      mountedRef.current = false
      conn.off('models_updated', handler)
    }
  }, [refetch])

  return {
    models,
    loading,
    error,
    refetch,
  }
}
