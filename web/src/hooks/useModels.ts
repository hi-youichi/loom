import { useState, useEffect, useCallback, useRef } from 'react'
import { getAvailableModels } from '../services/model'

export interface Model {
  id: string
  name: string
  provider: string
  family?: string
  capabilities?: string[]
}

export function useModels() {
  const [models, setModels] = useState<Model[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const mountedRef = useRef(true)

  const fetchModels = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const data = await getAvailableModels()
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

  return { models, loading, error, refetch: fetchModels }
}