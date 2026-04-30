import path from 'path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
    dedupe: [
      '@loom/ws-client',
      '@loom/protocol',
      '@loom/service-agent',
      '@loom/service-chat',
      '@loom/service-session',
      '@loom/service-workspace',
    ],
  },
  optimizeDeps: {
    include: [
      '@loom/ws-client',
      '@loom/protocol',
      '@loom/service-agent',
      '@loom/service-chat',
      '@loom/service-session',
      '@loom/service-workspace',
    ],
  },
})
