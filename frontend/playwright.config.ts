import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, devices } from "@playwright/test";

// Layer 3b config (testing pyramid per ADR 0004). Boots the real daemon
// via `cargo run` and points the browser at it. Daemon spawns frontend
// build.rs work itself; in CI we set PDO_SKIP_FRONTEND_BUILD=1 around
// `pnpm run e2e` to avoid the redundant rebuild after the e2e job's build.

const PORT = Number(process.env.PDO_E2E_PORT ?? 5273);
const HOST = "127.0.0.1";
const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
// The binary the `webServer` below builds and runs is the `pdo` CLI itself: the
// stub's `waiter` branch calls it.
const PDO_BIN = path.join(
  process.env.CARGO_TARGET_DIR ? path.resolve(REPO_ROOT, process.env.CARGO_TARGET_DIR) : path.join(REPO_ROOT, "target"),
  "debug",
  "pdo",
);
/** What the e2e `waiter` node asks its user (waiting-alert.spec.ts). */
const E2E_WAITER_MESSAGE = "Which layout do you prefer?";

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  expect: { timeout: 5_000 },
  fullyParallel: false,
  // Every spec shares one real daemon and one SQLite database. Multiple workers
  // race on global runs, triggers, and pipeline watcher events.
  workers: 1,
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
    // #823: a daemon with no Run at all offers the guided tour in a modal that
    // covers the whole app — and a fresh e2e database has no Run until a spec
    // creates one. Mark the tour as already offered (`pdo.tour.offered`, see
    // `src/lib/tourMemory.ts`) so no spec meets the modal; none is about it.
    storageState: {
      cookies: [],
      origins: [
        {
          origin: `http://${HOST}:${PORT}`,
          // The UI scale at 100 %: the specs measure the interface in CSS
          // pixels as drawn at 1:1. The design-system spec covers the 120 %
          // the interface opens at by default (`lib/uiScale.ts`).
          localStorage: [
            { name: "pdo.tour.offered", value: "1" },
            { name: "pdo.ui.scale", value: "100" },
          ],
        },
      ],
    },
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    // This daemon owns a port-scoped tmux socket and must exercise production
    // recovery even when Playwright itself runs inside a PDO node.
    command:
      `env -u PDO_NODE_ID -u PDO_DAEMON_NO_CLEANUP ` +
      `cargo run -p pdo-daemon --quiet -- daemon --port ${PORT}`,
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
    //   - `waiter`    → after a beat (the node is running), declares a wait on
    //                   its user with the real CLI, `pdo wait-user --message …`,
    //                   then keeps the PTY alive (waiting-alert.spec.ts).
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
      // #427: the e2e daemon is the REAL binary, so it reads `PDO_PRICE_SYNC` from
      // the environment (armed by default). Disarm it: the suite must never depend
      // on models.dev being reachable, and CI must never egress for a UI test.
      PDO_PRICE_SYNC: "off",
      // Same for the boot version check: the vanilla channel asks the public
      // repository's releases, and this daemon shares the real home's cache.
      PDO_UPDATE_CHECK: "off",
      PDO_TMUX_CMD_OVERRIDE:
        'case "$PDO_NODE_ID" in ' +
        "scroller) " +
        "printf '\\033[?1049h\\033[?1h'; " +
        "i=1; while [ $i -le 80 ]; do printf 'MARKER_LINE_%d\\r\\n' \"$i\"; i=$((i+1)); done; " +
        "printf 'OUTPUT_DONE\\r\\n'; exec cat ;; " +
        "echoer) exec cat ;; " +
        `waiter) sleep 1; '${PDO_BIN}' wait-user --message '${E2E_WAITER_MESSAGE}'; exec sleep 600 ;; ` +
        "*) exec sleep 600 ;; " +
        "esac",
    },
  },
});
