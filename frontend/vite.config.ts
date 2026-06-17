import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

const daemonPort = process.env.PDO_PORT ?? '5172'
const daemonTarget = `http://127.0.0.1:${daemonPort}`

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    host: true,
    proxy: {
      '/ws': { target: daemonTarget, ws: true },
      '/sessions': { target: daemonTarget, ws: true },
      '/runs': daemonTarget,
      '/pipelines': daemonTarget,
      '/library': daemonTarget,
      '/repos': daemonTarget,
      '/triggers': daemonTarget,
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    exclude: ['e2e/**', 'node_modules/**'],
  },
})
