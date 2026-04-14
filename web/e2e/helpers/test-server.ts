/**
 * Test Server Manager for E2E Testing
 * 
 * Manages Mock LLM server and Rust backend server for E2E tests
 */

import { spawn, ChildProcess } from 'child_process'
import { startMockLLMServer, stopMockLLMServer, type http } from './mock-llm-server'
import os from 'os'
import path from 'path'
import { v4 as uuidv4 } from 'uuid'
import fs from 'fs'

export interface TestServerConfig {
  mockLLMPort?: number
  backendPort?: number
  workspaceDB?: string
}

export interface TestServers {
  mockLLMServer: http.Server | null
  backendProcess: ChildProcess | null
}

/**
 * Start all test servers
 */
export async function startTestServers(config: TestServerConfig = {}): Promise<TestServers> {
  const {
    mockLLMPort = 18080,
    backendPort = 8080,
    workspaceDB,
  } = config

  console.log('🚀 Starting test servers...')

  // 1. Start Mock LLM Server
  let mockLLMServer: http.Server | null = null
  try {
    mockLLMServer = await startMockLLMServer({
      port: mockLLMPort,
      responseDelay: 100,
    })
    console.log(`✅ Mock LLM server started on port ${mockLLMPort}`)
  } catch (error) {
    console.error('❌ Failed to start Mock LLM server:', error)
    throw error
  }

  // 2. Start Rust Backend Server
  let backendProcess: ChildProcess | null = null
  try {
    const backendEnv: Record<string, string> = {
      ...process.env,
      OPENAI_API_KEY: 'test-key',
      OPENAI_BASE_URL: `http://127.0.0.1:${mockLLMPort}/v1`,
      LLM_PROVIDER: 'openai',
      RUST_LOG: 'info',
      TEST_SERVER_PORT: backendPort.toString(),
    }

    if (workspaceDB) {
      backendEnv.WORKSPACE_DB = workspaceDB
    }

    // Run the test-server binary
    backendProcess = spawn('cargo', [
      'run',
      '--package', 'serve',
      '--features', 'test-server',
      '--bin', 'test-server'
    ], {
      stdio: 'inherit',
      cwd: '..',
      env: backendEnv,
    })

    console.log('✅ Rust backend server started')

    // Wait for backend to be ready
    await waitForBackend(backendPort, 10000)

  } catch (error) {
    console.error('❌ Failed to start Rust backend:', error)
    
    // Cleanup Mock LLM server if backend fails
    if (mockLLMServer) {
      await stopMockLLMServer(mockLLMServer)
    }
    
    throw error
  }

  console.log('✅ All test servers started successfully')

  return {
    mockLLMServer,
    backendProcess,
  }
}

/**
 * Stop all test servers
 */
export async function stopTestServers(servers: TestServers): Promise<void> {
  console.log('🧹 Stopping test servers...')

  // Stop Mock LLM Server
  if (servers.mockLLMServer) {
    try {
      await stopMockLLMServer(servers.mockLLMServer)
      console.log('✅ Mock LLM server stopped')
    } catch (error) {
      console.error('❌ Failed to stop Mock LLM server:', error)
    }
  }

  // Stop Rust Backend Server
  if (servers.backendProcess) {
    try {
      servers.backendProcess.kill()
      console.log('✅ Rust backend server stopped')
    } catch (error) {
      console.error('❌ Failed to stop Rust backend:', error)
    }
  }

  console.log('✅ All test servers stopped')
}

/**
 * Wait for backend server to be ready
 * Checks if WebSocket server is accepting connections
 */
async function waitForBackend(port: number, timeout: number): Promise<void> {
  const startTime = Date.now()
  const WebSocket = (await import('ws')).default
  
  while (Date.now() - startTime < timeout) {
    try {
      // Try to connect to WebSocket server
      const ws = new WebSocket(`ws://127.0.0.1:${port}`)
      
      await new Promise((resolve, reject) => {
        ws.on('open', () => {
          ws.close()
          resolve(undefined)
        })
        ws.on('error', reject)
        
        // Timeout after 1 second
        setTimeout(() => reject(new Error('Connection timeout')), 1000)
      })
      
      return // Connection successful
    } catch {
      // Server not ready yet, wait and retry
    }
    
    await new Promise(resolve => setTimeout(resolve, 500))
  }
  
  throw new Error(`Backend server not ready after ${timeout}ms`)
}

/**
 * Create temporary workspace database
 */
export function createTempWorkspaceDB(): string {
  const tempDir = os.tmpdir()
  const dbPath = path.join(tempDir, `loom-test-workspace-${uuidv4()}.db`)
  
  return dbPath
}

/**
 * Clean up temporary files
 */
export function cleanupTempFile(filePath: string): void {
  try {
    if (fs.existsSync(filePath)) {
      fs.unlinkSync(filePath)
      console.log(`✅ Cleaned up temporary file: ${filePath}`)
    }
  } catch (error) {
    console.error(`❌ Failed to clean up file ${filePath}:`, error)
  }
}

/**
 * Get WebSocket URL for testing
 */
export function getWebSocketURL(port: number = 8080): string {
  return `ws://127.0.0.1:${port}`
}

/**
 * Get Mock LLM URL for testing
 */
export function getMockLLMURL(port: number = 18080): string {
  return `http://127.0.0.1:${port}/v1`
}
