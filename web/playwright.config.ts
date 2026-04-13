import { defineConfig } from '@playwright/test'

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
  globalSetup: require.resolve('./e2e/global-setup.ts'),
  // Global teardown: stop test servers after all tests
  globalTeardown: require.resolve('./e2e/global-teardown.ts'),
})
