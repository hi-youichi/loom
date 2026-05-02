import fs from 'fs'
import path from 'path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const pkg = JSON.parse(fs.readFileSync(path.resolve(__dirname, 'package.json'), 'utf-8'))
const workspaceDeps = Object.keys(pkg.dependencies ?? {})
  .filter((name) => pkg.dependencies[name] === 'workspace:*')
  .sort()

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
    dedupe: workspaceDeps,
  },
  optimizeDeps: {
    include: workspaceDeps,
  },
})
