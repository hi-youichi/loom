/**
 * Global Setup for Playwright E2E Tests
 * 
 * This script runs once before all tests to start test servers
 */

import { startTestServers, createTempWorkspaceDB, getWebSocketURL, getMockLLMURL } from './helpers/test-server'

// Store server references globally for teardown
declare global {
  var __testServers: any
  var __workspaceDB: string
  var __webSocketURL: string
  var __mockLLMURL: string
}

export async function setup(config: any) {
  console.log('🌟 Playwright Global Setup')
  
  // Create temporary workspace database
  const workspaceDB = createTempWorkspaceDB()
  console.log(`📁 Created temporary workspace DB: ${workspaceDB}`)
  
  // Start test servers
  const testServers = await startTestServers({
    mockLLMPort: 18080,
    backendPort: 8080,
    workspaceDB,
  })
  
  // Store references globally
  global.__testServers = testServers
  global.__workspaceDB = workspaceDB
  global.__webSocketURL = getWebSocketURL(8080)
  global.__mockLLMURL = getMockLLMURL(18080)
  
  console.log('✅ Global setup completed')
  console.log(`🔗 WebSocket URL: ${global.__webSocketURL}`)
  console.log(`🔗 Mock LLM URL: ${global.__mockLLMURL}`)
  
  // Set environment variables for tests
  process.env.VITE_LOOM_WS_URL = global.__webSocketURL
  process.env.OPENAI_API_KEY = 'test-key'
  process.env.OPENAI_BASE_URL = global.__mockLLMURL
  process.env.LLM_PROVIDER = 'openai'
}
