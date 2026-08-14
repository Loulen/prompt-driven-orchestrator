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
      // #345: POST /nodes/parse — a top-level route, so it needs its own proxy
      // entry (else dev GET/POST lie: SPA 200 / POST 404).
      '/nodes': daemonTarget,
      '/repos': daemonTarget,
      '/triggers': daemonTarget,
      '/stale': daemonTarget,
      '/settings': daemonTarget,
      // #377: instance stats cockpit. New top-level `/stats` prefix, so it needs
      // its own proxy entry (else dev GET /stats/* lies with a SPA 200).
      '/stats': daemonTarget,
      // #431: generic filesystem explorer, renamed from `/repos/browse`. New
      // top-level `/fs` prefix, so it needs its own entry — same trap as `/nodes`
      // (#345) and `/stats` (#377): without it a dev GET /fs/browse answers 200
      // with the SPA and any smoke test would lie.
      '/fs': daemonTarget,
      // #507: out-of-Run audit feed. New top-level `/audit` prefix — same trap
      // as `/nodes`/`/stats`/`/fs`: without it a dev GET /audit answers 200 with
      // the SPA.
      '/audit': daemonTarget,
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    exclude: ['e2e/**', 'node_modules/**'],
  },
})
