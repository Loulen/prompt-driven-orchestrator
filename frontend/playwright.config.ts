import { defineConfig, devices } from "@playwright/test";

// Layer 3b config (testing pyramid per ADR 0004). Boots the real daemon
// via `cargo run` and points the browser at it. Daemon spawns frontend
// build.rs work itself; in CI we set PDO_SKIP_FRONTEND_BUILD=1 around
// `npm run e2e` to avoid the redundant rebuild after the e2e job's npm build.

const PORT = Number(process.env.PDO_E2E_PORT ?? 5273);
const HOST = "127.0.0.1";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  expect: { timeout: 5_000 },
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: `http://${HOST}:${PORT}`,
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    command: `cargo run -p pdo-daemon --quiet -- daemon --port ${PORT}`,
    cwd: "..",
    url: `http://${HOST}:${PORT}/runs`,
    timeout: 180_000,
    reuseExistingServer: !process.env.CI,
    stdout: "pipe",
    stderr: "pipe",
    // The daemon spawns a tmux session per node. In production that runs the
    // real `claude` CLI; in e2e we MUST stub it so a run-creating test never
    // launches (and leaks) a real Claude process — and so node-session tests
    // are deterministic on CI where `claude` isn't installed. This env reaches
    // the DAEMON (the per-spec `process.env.PDO_TMUX_CMD_OVERRIDE=...` lines run
    // in the Playwright worker and never reached the already-spawned daemon, so
    // they were no-ops). `sleep 600` keeps the node "running" with a live PTY.
    env: { ...process.env, PDO_TMUX_CMD_OVERRIDE: "exec sleep 600" },
  },
});
