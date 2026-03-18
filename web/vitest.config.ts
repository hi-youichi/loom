import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { coverageConfigDefaults } from 'vitest/config'

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./src/__tests__/setup.ts'],
    css: true,
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json', 'html', 'lcov'],
      exclude: [
        ...coverageConfigDefaults.exclude,
        'src/main.tsx',
        'src/App.tsx',
        'src/**/*.d.ts',
        'src/__tests__/**',
        'src/types/**',
        'src/**/*.css',
      ],
      thresholds: {
        lines: 90,
        functions: 90,
        branches: 85,
        statements: 90
      }
    }
  }
})
