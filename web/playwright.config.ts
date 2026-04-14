import { defineConfig } from '@playwright/test'
import { fileURLToPath } from 'url'
import { dirname, resolve } from 'path'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)

export default defineConfig({
  testDir: './e2e',
  timeout: 60000, // Increased timeout for server startup
  retries: 0,
  use: {
    baseURL: 'http://localhost:5173',
    headless: true,
    actionTimeout: 10000,
  },
  expect: {
    timeout: 10000,
  },
  projects: [
    {
      name: 'chromium',
      use: { browserName: 'chromium' },
    },
  ],
  // Global setup: start test servers before all tests
  globalSetup: resolve(__dirname, './e2e/global-setup.ts'),
  // Global teardown: stop test servers after all tests  
  globalTeardown: resolve(__dirname, './e2e/global-teardown.ts'),
})
