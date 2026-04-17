/**
 * Mock LLM Server for E2E Testing
 * 
 * This server simulates OpenAI-compatible API responses
 * to avoid calling real LLM APIs during testing.
 */

import http from 'http'

export interface MockLLMRequest {
  model: string
  messages: Array<{
    role: string
    content: string
  }>
  temperature?: number
  max_tokens?: number
  stream?: boolean
}

export interface MockLLMResponse {
  id: string
  object: string
  created: number
  model: string
  choices: Array<{
    index: number
    message: {
      role: string
      content: string
    }
    finish_reason: string
  }>
  usage?: {
    prompt_tokens: number
    completion_tokens: number
    total_tokens: number
  }
}

export interface MockLLMServerConfig {
  port: number
  responseDelay?: number // Response delay in ms
  customResponse?: (req: MockLLMRequest) => string
}

/**
 * Start Mock LLM Server
 */
export function startMockLLMServer(config: MockLLMServerConfig): Promise<http.Server> {
  const { port, responseDelay = 100, customResponse } = config
  const server = http.createServer((req, res) => {
    // Enable CORS
    res.setHeader('Access-Control-Allow-Origin', '*')
    res.setHeader('Access-Control-Allow-Methods', 'POST, OPTIONS')
    res.setHeader('Access-Control-Allow-Headers', 'Content-Type, Authorization')

    if (req.method === 'OPTIONS') {
      res.writeHead(204)
      res.end()
      return
    }

    if (req.url?.includes('/models')) {
      res.writeHead(200, { 'Content-Type': 'application/json' })
      res.end(JSON.stringify({
        object: 'list',
        data: [
          { id: 'gpt-4o-mini', object: 'model', created: 1, owned_by: 'mock' },
        ],
      }))
      return
    }

    if (req.method !== 'POST') {
      res.writeHead(405, { 'Content-Type': 'application/json' })
      res.end(JSON.stringify({ error: 'Method not allowed' }))
      return
    }

    let body = ''
    req.on('data', chunk => { body += chunk })
    req.on('end', () => {
      try {
        const requestBody: MockLLMRequest = JSON.parse(body)
        
        // Generate response
        let responseContent: string
        
        if (customResponse) {
          responseContent = customResponse(requestBody)
        } else {
          responseContent = generateDefaultResponse(requestBody)
        }

        const response: MockLLMResponse = {
          id: `chatcmpl-${Math.random().toString(36).substr(2, 9)}`,
          object: 'chat.completion',
          created: Math.floor(Date.now() / 1000),
          model: requestBody.model || 'test-model',
          choices: [{
            index: 0,
            message: {
              role: 'assistant',
              content: responseContent,
            },
            finish_reason: 'stop',
          }],
          usage: {
            prompt_tokens: requestBody.messages.reduce((sum, msg) => sum + msg.content.length, 0),
            completion_tokens: responseContent.length,
            total_tokens: 0, // Will be calculated below
          }
        }
        
        response.usage.total_tokens = response.usage.prompt_tokens + response.usage.completion_tokens

        // Simulate network delay
        setTimeout(() => {
          if (requestBody.stream) {
            res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache', 'Connection': 'keep-alive' })

            const chunkId = response.id
            const model = response.model

            const sseChunks = [
              JSON.stringify({
                id: chunkId,
                object: 'chat.completion.chunk',
                created: Math.floor(Date.now() / 1000),
                model,
                choices: [{ index: 0, delta: { role: 'assistant' }, finish_reason: null }],
              }),
              JSON.stringify({
                id: chunkId,
                object: 'chat.completion.chunk',
                created: Math.floor(Date.now() / 1000),
                model,
                choices: [{ index: 0, delta: { content: responseContent }, finish_reason: null }],
              }),
              JSON.stringify({
                id: chunkId,
                object: 'chat.completion.chunk',
                created: Math.floor(Date.now() / 1000),
                model,
                choices: [{ index: 0, delta: {}, finish_reason: 'stop' }],
                usage: response.usage,
              }),
            ]

            for (const chunk of sseChunks) {
              res.write(`data: ${chunk}\n\n`)
            }

            res.write('data: [DONE]\n\n')
            res.end()
          } else {
            res.writeHead(200, { 'Content-Type': 'application/json' })
            res.end(JSON.stringify(response))
          }
        }, responseDelay)
        
      } catch (error) {
        res.writeHead(400, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify({ error: 'Invalid request body' }))
      }
    })
  })

  return new Promise((resolve) => {
    server.listen(port, () => {
      console.log(`Mock LLM server running on http://localhost:${port}`)
      resolve(server)
    })
  })
}

/**
 * Generate default response based on user message
 */
function generateDefaultResponse(request: MockLLMRequest): string {
  const lastMessage = request.messages[request.messages.length - 1]
  
  if (lastMessage.role === 'user') {
    const userContent = lastMessage.content.toLowerCase()
    
    if (userContent.includes('hello') || userContent.includes('hi')) {
      return '{"completed": true, "reason": "Greeting received"}'
    } else if (userContent.includes('test')) {
      return '{"completed": true, "reason": "Test completed"}'
    } else if (userContent.includes('error')) {
      return '{"completed": true, "reason": "Error simulated"}'
    } else {
      return '{"completed": true, "reason": "Task finished"}'
    }
  }
  
  return '{"completed": true, "reason": "Done"}'
}

/**
 * Stop Mock LLM Server
 */
export function stopMockLLMServer(server: http.Server): Promise<void> {
  return new Promise((resolve) => {
    server.close(() => {
      console.log('Mock LLM server stopped')
      resolve()
    })
  })
}

/**
 * Get server URL
 */
export function getMockLLMURL(port: number): string {
  return `http://localhost:${port}/v1`
}
