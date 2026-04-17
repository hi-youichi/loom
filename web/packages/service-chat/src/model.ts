import { getConnection } from '@graphweave/ws-client'
import type { Model } from '@graphweave/ws-client'

export type { Model }

export interface ListModelsRequest {
  type: 'list_models'
  id: string
}

export interface ListModelsResponse {
  type: 'models_list'
  id: string
  models: Model[]
}

export interface SetModelRequest {
  type: 'set_model'
  id: string
  model_id: string
  session_id?: string
}

export interface SetModelResponse {
  type: 'model_set'
  id: string
  success: boolean
  error?: string
}

export async function getAvailableModels(): Promise<Model[]> {
  const requestId = crypto.randomUUID()
  const request: ListModelsRequest = {
    type: 'list_models',
    id: requestId,
  }

  try {
    const response = await getConnection().request(request) as ListModelsResponse
    const models = response.models || []

    if (models.length === 0) {
      throw new Error('No models available from server')
    }

    return models
  } catch (error) {
    console.error('Failed to get available models:', error)
    throw error
  }
}

export async function setSessionModel(
  modelId: string,
  sessionId?: string
): Promise<boolean> {
  const requestId = crypto.randomUUID()
  const request: SetModelRequest = {
    type: 'set_model',
    id: requestId,
    model_id: modelId,
    session_id: sessionId,
  }

  try {
    const response = await getConnection().request(request) as SetModelResponse
    return response.success || false
  } catch (error) {
    console.error('Failed to set session model:', error)
    return false
  }
}

export async function isModelAvailable(modelId: string): Promise<boolean> {
  try {
    const models = await getAvailableModels()
    return models.some(model => model.id === modelId)
  } catch (error) {
    console.error('Failed to check model availability:', error)
    return false
  }
}
