import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — xterm.js wheel-scroll (refs #73, ADR 0005).
// Verifies:
// 1. Wheel-up on the inline terminal scrolls the xterm buffer (history visible).
// 2. Wheel events do NOT produce arrow-key sequences in the TTY.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-scroll-wheel-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: scroller
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 100, y: 100 }
edges: []
`;

// Shell script that emits numbered marker lines then waits for input,
// logging any received keystrokes so we can assert no arrow-key escapes arrived.
const MARKER_SCRIPT = `exec sh -c '
  i=1; while [ $i -le 80 ]; do echo "MARKER_LINE_$i"; i=$((i+1)); done
  echo "OUTPUT_DONE"
  # Read stdin and log any bytes received (arrow keys show as escape sequences)
  while IFS= read -r line; do echo "KEYSTROKE:$line"; done
'`;

test.beforeAll(async () => {
  process.env.MAESTRO_TMUX_CMD_OVERRIDE = MARKER_SCRIPT;
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  delete process.env.MAESTRO_TMUX_CMD_OVERRIDE;
});

test("wheel-up scrolls xterm buffer to show earlier output", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const resp = await page.request.post(`${baseURL}/runs`, {
    data: {
      pipeline: PIPELINE_NAME,
      input: "e2e scroll wheel test",
    },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();

  await page.getByText(run_id.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  const workerNode = page.getByText("scroller", { exact: true }).first();
  await expect(workerNode).toBeVisible({ timeout: 3_000 });
  await workerNode.click();

  const terminal = page.getByTestId("tmux-terminal");
  await expect(terminal).toBeVisible({ timeout: 5_000 });

  const xtermContainer = page.getByTestId("xterm-container");
  await expect(xtermContainer.locator("canvas").first()).toBeVisible({
    timeout: 5_000,
  });

  // Wait for output to finish rendering
  await expect(async () => {
    const text = await page.evaluate(() => {
      const rows = document.querySelector(
        '[data-testid="xterm-container"] .xterm-rows',
      );
      return rows?.textContent ?? "";
    });
    expect(text).toContain("OUTPUT_DONE");
  }).toPass({ timeout: 8_000 });

  // Capture text visible before scrolling (should be near the end)
  const textBefore = await page.evaluate(() => {
    const rows = document.querySelector(
      '[data-testid="xterm-container"] .xterm-rows',
    );
    return rows?.textContent ?? "";
  });
  expect(textBefore).toContain("MARKER_LINE_80");

  // Scroll up via wheel event on the xterm container
  await xtermContainer.dispatchEvent("wheel", {
    deltaY: -500,
  });
  await page.waitForTimeout(300);

  // After scrolling up, earlier marker lines should be visible
  const textAfter = await page.evaluate(() => {
    const rows = document.querySelector(
      '[data-testid="xterm-container"] .xterm-rows',
    );
    return rows?.textContent ?? "";
  });
  expect(textAfter).toContain("MARKER_LINE_1");

  // Verify no arrow-key keystrokes were sent to the TTY
  await page.waitForTimeout(500);
  const finalText = await page.evaluate(() => {
    const rows = document.querySelector(
      '[data-testid="xterm-container"] .xterm-rows',
    );
    return rows?.textContent ?? "";
  });
  expect(finalText).not.toContain("KEYSTROKE:");

  // Cleanup tmux session
  const sessionName = `maestro-${run_id}-scroller-iter-1`;
  const { execSync } = await import("node:child_process");
  try {
    execSync(`tmux kill-session -t ${sessionName}`, { stdio: "ignore" });
  } catch {
    // session may already be dead
  }
});
