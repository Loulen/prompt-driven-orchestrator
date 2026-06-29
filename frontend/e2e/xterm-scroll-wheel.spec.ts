import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { cleanupRuns } from "./helpers";

// Layer 3b — xterm.js wheel-scroll regression (refs #73, ADR 0005).
//
// The bug being guarded against: in alt-screen + Application Cursor Mode
// (DECCKM) — the standard mode for any TUI we host (Claude Code, vim, less) —
// xterm.js's own viewport wheel handler translates mouse-wheel events into
// arrow-key escape sequences (`ESC O A` / `ESC O B`) and pushes them straight
// to the PTY via `term.onData`. From the user's POV the terminal scrolls the
// wrong thing (cursor inside the underlying app) and there is no way to read
// scrollback. The fix registers our own wheel listener in **capture** phase
// on the container so it preempts xterm's viewport handler, calls
// `stopImmediatePropagation`, and either scrolls the xterm normal-screen
// buffer or — in alt-screen mode where there is no scrollback — does nothing
// (silent suppression).

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-scroll-wheel-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

// Post-refonte the parser requires exactly one start node (zero inputs, one
// `user_prompt` output) and one end node (zero outputs, one `result` input).
// The node id is `scroller` so the daemon-wide stub dispatcher
// (playwright.config.ts) switches its PTY into alt-screen (`ESC[?1049h`) +
// Application Cursor Mode (`ESC[?1h`), prints 80 `MARKER_LINE_*` rows then
// `OUTPUT_DONE`, and blocks on `cat`. That is the exact mode configuration
// where xterm.js's wheel→arrow-key path activates; without these escapes a
// plain `sleep` stub just scrolls normal-screen scrollback and the regression
// hides.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
    view: { x: 100, y: 0 }
  - id: scroller
    name: scroller
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 100, y: 150 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    view: { x: 100, y: 300 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: scroller, port: in }
  - source: { node: scroller, port: out }
    target: { node: end, port: result }
`;

const createdRunIds: string[] = [];

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await cleanupRuns(...createdRunIds);
});

test("wheel inside alt-screen xterm does not leak arrow-key bytes to the PTY", async ({
  page,
  baseURL,
}) => {
  // Capture every byte the page writes to any WebSocket. Installed via
  // addInitScript so the patch runs before the xterm WebSocket is created.
  await page.addInitScript(() => {
    const w = window as unknown as { __wsBytes: number[] };
    w.__wsBytes = [];
    const origSend = WebSocket.prototype.send;
    WebSocket.prototype.send = function (
      data: string | ArrayBufferLike | Blob | ArrayBufferView,
    ) {
      let bytes: number[] | null = null;
      if (data instanceof ArrayBuffer) {
        bytes = Array.from(new Uint8Array(data));
      } else if (ArrayBuffer.isView(data)) {
        bytes = Array.from(
          new Uint8Array(data.buffer, data.byteOffset, data.byteLength),
        );
      }
      if (bytes) w.__wsBytes.push(...bytes);
      return origSend.call(this, data);
    };
  });

  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: {
      pipeline: PIPELINE_NAME,
      input: "e2e scroll wheel test",
    },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();
  createdRunIds.push(run_id);

  await page.getByText(run_id.slice(0, 20)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  const workerNode = page.getByText("scroller", { exact: true }).first();
  await expect(workerNode).toBeVisible({ timeout: 3_000 });
  await workerNode.click();

  const terminal = page.getByTestId("tmux-terminal");
  await expect(terminal).toBeVisible({ timeout: 5_000 });

  // xterm.js (v6, no canvas/webgl addon) uses the DOM renderer, so the rendered
  // screen is `.xterm-screen`/`.xterm-rows`, not a <canvas>.
  const xtermContainer = page.getByTestId("xterm-container");
  await expect(xtermContainer.locator(".xterm-screen").first()).toBeVisible({
    timeout: 5_000,
  });

  // Wait for the script's marker output to render — this also confirms the
  // PTY successfully entered alt-screen.
  await expect(async () => {
    const text = await page.evaluate(() => {
      const rows = document.querySelector(
        '[data-testid="xterm-container"] .xterm-rows',
      );
      return rows?.textContent ?? "";
    });
    expect(text).toContain("OUTPUT_DONE");
  }).toPass({ timeout: 8_000 });

  // Drop everything sent before the wheel so we only assert on wheel-induced
  // traffic (the resize handshake and any keystrokes during navigation are
  // irrelevant to this test).
  await page.evaluate(() => {
    (window as unknown as { __wsBytes: number[] }).__wsBytes.length = 0;
  });

  // Real user wheels land on the deepest xterm element, not on the outer
  // container. Dispatching here is what exercises xterm.js's viewport handler
  // (the one that used to emit the arrow-key escapes). The previous test
  // dispatched on the outer container, which bypassed xterm's handler entirely
  // and so silently passed even when the regression was live.
  const wheelTarget = page.locator(
    '[data-testid="xterm-container"] .xterm-screen, [data-testid="xterm-container"] .xterm-viewport',
  ).first();
  await expect(wheelTarget).toBeVisible({ timeout: 3_000 });

  for (let i = 0; i < 5; i++) {
    await wheelTarget.dispatchEvent("wheel", {
      deltaY: -100,
      bubbles: true,
      cancelable: true,
    });
  }
  await wheelTarget.dispatchEvent("wheel", {
    deltaY: 500,
    bubbles: true,
    cancelable: true,
  });
  await page.waitForTimeout(300);

  // ESC O A = [27, 79, 65] = up arrow under DECCKM
  // ESC O B = [27, 79, 66] = down arrow under DECCKM
  // Either appearing in the WS stream means xterm.js's wheel→arrow-key
  // translation reached the PTY — i.e. the regression is back.
  const bytes = (await page.evaluate(
    () => (window as unknown as { __wsBytes: number[] }).__wsBytes,
  )) as number[];

  const containsSequence = (haystack: number[], needle: number[]): boolean => {
    outer: for (let i = 0; i <= haystack.length - needle.length; i++) {
      for (let j = 0; j < needle.length; j++) {
        if (haystack[i + j] !== needle[j]) continue outer;
      }
      return true;
    }
    return false;
  };

  expect(
    containsSequence(bytes, [27, 79, 65]),
    "wheel-up emitted ESC O A (up arrow) to the PTY — xterm.js wheel handler ran",
  ).toBe(false);
  expect(
    containsSequence(bytes, [27, 79, 66]),
    "wheel-down emitted ESC O B (down arrow) to the PTY — xterm.js wheel handler ran",
  ).toBe(false);

  const sessionName = `pdo-${run_id}-scroller-iter-1`;
  const { execSync } = await import("node:child_process");
  try {
    execSync(`tmux kill-session -t ${sessionName}`, { stdio: "ignore" });
  } catch {
    // session may already be dead
  }
});
