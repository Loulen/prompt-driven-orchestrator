import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

const daemonPort = process.env.PDO_PORT ?? '5172'
const daemonTarget = `http://127.0.0.1:${daemonPort}`

export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    // crates/pdo/build.rs sets PDO_FRONTEND_OUT_DIR to a folder under cargo's
    // OUT_DIR, the one the daemon embeds (#21): a cargo build never writes
    // `dist/` into the source tree. Vite only empties an outDir outside the
    // project root when told to.
    outDir: process.env.PDO_FRONTEND_OUT_DIR ?? 'dist',
    emptyOutDir: true,
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    host: true,
    // Every top-level path prefix the daemon serves needs its entry here: the
    // router is assembled in `crates/pdo/src/contexts/mod.rs`, one `router()`
    // per bounded context (ADR-0079). A prefix missing here answers 200 with the
    // SPA shell in `make dev`, and any smoke test lies.
    proxy: {
      // #564: the daemon checks the WebSocket `Origin` on BOTH WS routes as a
      // DNS-rebinding / CSWSH guard. In `make dev` the browser's Origin is the
      // Vite dev server (`:5173`), not the daemon, so without rewriting it the
      // daemon answers 403 and the dashboard/terminal go dark. `rewriteWsOrigin`
      // sets the upstream Origin to the proxy target so the check passes — the
      // dev-only counterpart to `PDO_ALLOWED_WS_ORIGINS` in prod, and it never
      // touches the shipped binary's defaults. (This also fixes the PTY terminal,
      // which was silently broken under Vite dev for the same reason.)
      '/ws': { target: daemonTarget, ws: true, rewriteWsOrigin: true },
      '/sessions': { target: daemonTarget, ws: true, rewriteWsOrigin: true },
      '/runs': daemonTarget,
      // ADR-0088: `/pipelines/{id}/draft` is the shared draft's WebSocket.
      '/pipelines': { target: daemonTarget, ws: true, rewriteWsOrigin: true },
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
      '/pages': daemonTarget,
      // #552: Projets (harness middle tier + group-header pencil). New top-level
      // `/projects` prefix — same trap as `/audit`: without it a dev
      // GET/POST /projects answers 200 with the SPA and the pencil would silently
      // no-op.
      '/projects': daemonTarget,
      // #697: version check (`GET /update`, `POST /update/check`). New top-level
      // prefix — same trap as the others: without it a dev GET /update answers 200
      // with the SPA and the section would render nothing, silently.
      '/update': daemonTarget,
      // ADR-0081 (#931): the extensions. `GET /ext` lists them and their UI
      // slots, `/ext/<id>/…` are each extension's own routes (its UI bundle
      // included). Same trap as the others: without it a dev GET /ext answers
      // 200 with the SPA and no extension ever shows up.
      '/ext': daemonTarget,
      // 0.8: who the UI is (`GET /actors/me`) and the Actors' display names
      // (`POST /actors/display`). Same trap: without it the sign-in gate reads
      // the SPA shell and never redirects.
      '/actors': daemonTarget,
      // CORE-3: `GET /resources`, the catalogue of the Ressources an extension
      // of rights reads. Same trap: without it a dev GET answers the SPA.
      '/resources': daemonTarget,
      // CORE-5: `POST /authorize`, the question of authorization (hides the
      // loupe and the Projets a Run cannot be created in).
      '/authorize': daemonTarget,
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    exclude: ['e2e/**', 'node_modules/**'],
    // Heavy App/EditCanvas jsdom renders routinely exceed vitest's 5s default
    // when the suite runs on a loaded machine (parallel workers); the failures
    // were pure timeouts moving between unrelated heavy tests run to run.
    // 20s matches the per-test bump already used for the mermaid import test.
    testTimeout: 20_000,
  },
})
