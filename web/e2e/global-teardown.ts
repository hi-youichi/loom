/**
 * Global Teardown for Playwright E2E Tests
 * 
 * This script runs once after all tests to stop test servers
 */

import { stopTestServers, cleanupTempFile } from './helpers/test-server'

declare global {
  var __testServers: any
  var __workspaceDB: string
}

export async function teardown(config: any) {
  console.log('🌟 Playwright Global Teardown')
  
  // Stop test servers
  if (global.__testServers) {
    await stopTestServers(global.__testServers)
  }
  
  // Clean up temporary files
  if (global.__workspaceDB) {
    cleanupTempFile(global.__workspaceDB)
  }
  
  console.log('✅ Global teardown completed')
}
