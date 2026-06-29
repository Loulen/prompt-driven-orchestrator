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
  // Layer 3b drives a real daemon + tmux + browser, so a test can fail on a
  // transient (a slow session spawn, an SSE lag). Retry in CI to keep the gate
  // on genuine regressions; locally stay strict (retries:0) so flakes surface
  // during development.
  retries: process.env.CI ? 2 : 0,
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
    // they were no-ops).
    //
    // #218 bucket D — a *single* daemon serves every spec by dispatching on
    // `PDO_NODE_ID` (the daemon exports it into each node session before running
    // this override — see tmux_session_manager::wrap_with_env). Two daemons would
    // share `<repo_root>/.pdo/pdo.db` and contend on SQLite, so we dispatch in the
    // stub instead of standing up a second `webServer` + project. This stays
    // entirely in the test harness — no product API (honours ADR-0009 vs a
    // runtime per-run node-command surface):
    //   - `scroller`  → alt-screen + Application Cursor Mode + 80 markers, then
    //                   block on `cat`. The exact mode where xterm.js's wheel→
    //                   arrow-key translation activates (xterm-scroll-wheel).
    //   - `echoer`    → `cat`, so typed input echoes back through the PTY bridge
    //                   (terminal-preview's keystroke-echo assertion).
    //   - everything else → `sleep 600`, a live PTY with the node kept "running".
    env: {
      ...process.env,
      // The global session cap counts the live NodeRun sessions across every
      // run in the daemon's DB (admission::count_live_node_sessions). The suite
      // creates many runs whose stub nodes stay "running" (the `sleep` branch
      // below); specs that create runs archive them in `afterAll`
      // (helpers.ts `cleanupRun`) so the live count stays near zero and never
      // approaches the default cap. A modest bump gives headroom for the
      // handful of runs a single spec holds open at once without lifting the
      // tmux-protecting cap so high that a missed cleanup could pile up.
      PDO_SESSION_CAP: "64",
      PDO_TMUX_CMD_OVERRIDE:
        'case "$PDO_NODE_ID" in ' +
        "scroller) " +
        "printf '\\033[?1049h\\033[?1h'; " +
        "i=1; while [ $i -le 80 ]; do printf 'MARKER_LINE_%d\\r\\n' \"$i\"; i=$((i+1)); done; " +
        "printf 'OUTPUT_DONE\\r\\n'; exec cat ;; " +
        "echoer) exec cat ;; " +
        "*) exec sleep 600 ;; " +
        "esac",
    },
  },
});
