import { defineConfig, devices } from "@playwright/test";

// Layer 3b config (testing pyramid per ADR 0004). Boots the real daemon
// via `cargo run` and points the browser at it. Daemon spawns frontend
// build.rs work itself; in CI we set MAESTRO_SKIP_FRONTEND_BUILD=1 around
// `npm run e2e` to avoid the redundant rebuild after the e2e job's npm build.

const PORT = Number(process.env.MAESTRO_E2E_PORT ?? 5273);
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
    command: `cargo run -p maestro-daemon --quiet -- daemon --port ${PORT}`,
    cwd: "..",
    url: `http://${HOST}:${PORT}/runs`,
    timeout: 180_000,
    reuseExistingServer: !process.env.CI,
    stdout: "pipe",
    stderr: "pipe",
  },
});
