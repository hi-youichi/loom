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
  loomHome: string
}

function createTestLoomHome(mockLLMPort: number): string {
  const tempDir = path.join(os.tmpdir(), `loom-test-home-${uuidv4()}`)
  fs.mkdirSync(tempDir, { recursive: true })

  const configToml = `
[[providers]]
name = "alibaba-cn"
api_key = "test-key"
base_url = "http://127.0.0.1:${mockLLMPort}/v1"
fetch_models = false

[[providers]]
name = "openrouter"
api_key = "test-key"
base_url = "http://127.0.0.1:${mockLLMPort}/v1"
fetch_models = false

[[providers]]
name = "zhipuai-coding-plan"
api_key = "test-key"
base_url = "http://127.0.0.1:${mockLLMPort}/v1"
fetch_models = false

[[providers]]
name = "moonshotai-cn"
api_key = "test-key"
base_url = "http://127.0.0.1:${mockLLMPort}/v1"
fetch_models = false
`
  fs.writeFileSync(path.join(tempDir, 'config.toml'), configToml)

  return tempDir
}

export async function startTestServers(config: TestServerConfig = {}): Promise<TestServers> {
  const {
    mockLLMPort = 18080,
    backendPort = 8080,
    workspaceDB,
  } = config

  console.log('Starting test servers...')

  let mockLLMServer: http.Server | null = null
  try {
    mockLLMServer = await startMockLLMServer({
      port: mockLLMPort,
      responseDelay: 100,
    })
    console.log(`Mock LLM server started on port ${mockLLMPort}`)
  } catch (error) {
    console.error('Failed to start Mock LLM server:', error)
    throw error
  }

  let backendProcess: ChildProcess | null = null
  const loomHome = createTestLoomHome(mockLLMPort)
  console.log(`Created test LOOM_HOME: ${loomHome}`)

  try {
    const backendEnv: Record<string, string> = {
      ...process.env as Record<string, string>,
      OPENAI_API_KEY: 'test-key',
      OPENAI_BASE_URL: `http://127.0.0.1:${mockLLMPort}/v1`,
      LLM_PROVIDER: 'openai',
      RUST_LOG: 'info',
      TEST_SERVER_PORT: backendPort.toString(),
      LOOM_HOME: loomHome,
    }

    if (workspaceDB) {
      backendEnv.WORKSPACE_DB = workspaceDB
    }

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

    console.log('Rust backend server started')

    await waitForBackend(backendPort, 30000)

  } catch (error) {
    console.error('Failed to start Rust backend:', error)

    if (mockLLMServer) {
      await stopMockLLMServer(mockLLMServer)
    }

    throw error
  }

  console.log('All test servers started successfully')

  return {
    mockLLMServer,
    backendProcess,
    loomHome,
  }
}

export async function stopTestServers(servers: TestServers): Promise<void> {
  console.log('Stopping test servers...')

  if (servers.mockLLMServer) {
    try {
      await stopMockLLMServer(servers.mockLLMServer)
      console.log('Mock LLM server stopped')
    } catch (error) {
      console.error('Failed to stop Mock LLM server:', error)
    }
  }

  if (servers.backendProcess) {
    try {
      servers.backendProcess.kill()
      console.log('Rust backend server stopped')
    } catch (error) {
      console.error('Failed to stop Rust backend:', error)
    }
  }

  console.log('All test servers stopped')
}

async function waitForBackend(port: number, timeout: number): Promise<void> {
  const startTime = Date.now()
  const WebSocket = (await import('ws')).default

  while (Date.now() - startTime < timeout) {
    try {
      const ws = new WebSocket(`ws://127.0.0.1:${port}`)

      await new Promise((resolve, reject) => {
        ws.on('open', () => {
          ws.close()
          resolve(undefined)
        })
        ws.on('error', reject)

        setTimeout(() => reject(new Error('Connection timeout')), 1000)
      })

      return
    } catch {
    }

    await new Promise(resolve => setTimeout(resolve, 500))
  }

  throw new Error(`Backend server not ready after ${timeout}ms`)
}

export function createTempWorkspaceDB(): string {
  const tempDir = os.tmpdir()
  const dbPath = path.join(tempDir, `loom-test-workspace-${uuidv4()}.db`)

  return dbPath
}

export function cleanupTempFile(filePath: string): void {
  try {
    if (fs.existsSync(filePath)) {
      const stat = fs.statSync(filePath)
      if (stat.isDirectory()) {
        fs.rmSync(filePath, { recursive: true, force: true })
      } else {
        fs.unlinkSync(filePath)
      }
      console.log(`Cleaned up: ${filePath}`)
    }
  } catch (error) {
    console.error(`Failed to clean up ${filePath}:`, error)
  }
}

export function getWebSocketURL(port: number = 8080): string {
  return `ws://127.0.0.1:${port}`
}

export function getMockLLMURL(port: number = 18080): string {
  return `http://127.0.0.1:${port}/v1`
}
