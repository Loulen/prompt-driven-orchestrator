import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

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
      '/ws': { target: 'http://127.0.0.1:5172', ws: true },
      '/sessions': { target: 'http://127.0.0.1:5172', ws: true },
      '/runs': 'http://127.0.0.1:5172',
      '/pipelines': 'http://127.0.0.1:5172',
      '/library': 'http://127.0.0.1:5172',
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    exclude: ['e2e/**', 'node_modules/**'],
  },
})
