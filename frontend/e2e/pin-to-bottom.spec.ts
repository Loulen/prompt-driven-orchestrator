import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 4 — Pin-to-bottom + chevron resume (#34).
// Verifies:
// 1. Scrolling up pauses terminal rendering and shows the chevron.
// 2. Clicking the chevron resumes rendering and scrolls to bottom.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-pin-to-bottom-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: worker
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 100, y: 100 }
edges: []
`;

test.beforeAll(async () => {
  process.env.MAESTRO_TMUX_CMD_OVERRIDE = "exec sleep 300";
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  delete process.env.MAESTRO_TMUX_CMD_OVERRIDE;
});

test("scroll-up pauses rendering and shows chevron; click resumes", async ({
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
      input: "e2e pin-to-bottom test",
    },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();

  await page.getByText(run_id.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  const workerNode = page.getByText("worker", { exact: true }).first();
  await expect(workerNode).toBeVisible({ timeout: 3_000 });
  await workerNode.click();

  const terminalPane = page.locator(".terminal-pane");
  await expect(terminalPane).toBeVisible({ timeout: 3_000 });

  // Wait for pane to have content
  await expect(async () => {
    const html = await terminalPane.innerHTML();
    expect(html.length).toBeGreaterThan(0);
  }).toPass({ timeout: 5_000 });

  // Chevron should not be visible when pinned to bottom
  const chevron = page.locator(".pin-bottom-chevron");
  await expect(chevron).not.toBeVisible();

  // Scroll the terminal pane up to simulate user scrolling away from bottom
  await terminalPane.evaluate((el) => {
    el.scrollTop = 0;
  });

  // Chevron should appear
  await expect(chevron).toBeVisible({ timeout: 2_000 });

  // Click chevron — should scroll back and hide itself
  await chevron.click();
  await expect(chevron).not.toBeVisible({ timeout: 2_000 });

  // Verify we're scrolled to the bottom
  const isAtBottom = await terminalPane.evaluate((el) => {
    return el.scrollHeight - el.scrollTop - el.clientHeight < 8;
  });
  expect(isAtBottom).toBe(true);

  // Cleanup
  const sessionName = `maestro-${run_id}-worker-iter-1`;
  const { execSync } = await import("node:child_process");
  try {
    execSync(`tmux kill-session -t ${sessionName}`, { stdio: "ignore" });
  } catch {
    // session may already be dead
  }
});
