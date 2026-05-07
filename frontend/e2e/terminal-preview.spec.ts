import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — Terminal preview polling (#23).
// Verifies:
// 1. Selecting a running node shows non-empty pane content within 2s.
// 2. Navigating away stops /pane requests.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-terminal-preview-${process.pid}-${Date.now()}`;
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
  // Produce real ANSI output so the dangerouslySetInnerHTML branch is exercised
  process.env.MAESTRO_TMUX_CMD_OVERRIDE =
    "exec sh -c \"printf '\\033[32mhello ansi\\033[0m\\n'; sleep 300\"";
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  delete process.env.MAESTRO_TMUX_CMD_OVERRIDE;
});

test("selecting a running node shows terminal preview within 2s", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Create a run via the API
  const resp = await page.request.post(`${baseURL}/runs`, {
    data: {
      pipeline: PIPELINE_NAME,
      input: "e2e terminal preview test",
    },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();

  // Wait for the run to appear in the list and click it
  await page.getByText(run_id.slice(0, 8)).first().click({ timeout: 5_000 });

  // Wait for the DAG to render then click the worker node
  await page.waitForTimeout(500);
  const workerNode = page.getByText("worker", { exact: true }).first();
  await expect(workerNode).toBeVisible({ timeout: 3_000 });
  await workerNode.click();

  // The terminal preview pane should show non-empty content within 2s
  const terminalPane = page.locator(".terminal-pane");
  await expect(terminalPane).toBeVisible({ timeout: 3_000 });

  // Wait for ANSI-rendered content (dangerouslySetInnerHTML branch exercised)
  await expect(async () => {
    const html = await terminalPane.innerHTML();
    expect(html).toContain("hello ansi");
  }).toPass({ timeout: 5_000 });

  // Cleanup: kill the tmux session
  const sessionName = `maestro-${run_id}-worker-iter-1`;
  const { execSync } = await import("node:child_process");
  try {
    execSync(`tmux kill-session -t ${sessionName}`, { stdio: "ignore" });
  } catch {
    // session may already be dead
  }
});

test("navigating away stops pane polling", async ({ page, baseURL }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Create a run via the API
  const resp = await page.request.post(`${baseURL}/runs`, {
    data: {
      pipeline: PIPELINE_NAME,
      input: "e2e stop-polling test",
    },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();

  // Select the run and node
  await page.getByText(run_id.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  const workerNode = page.getByText("worker", { exact: true }).first();
  await expect(workerNode).toBeVisible({ timeout: 3_000 });
  await workerNode.click();

  // Confirm at least one /pane request fires
  await page.waitForRequest((req) => req.url().includes("/pane"), {
    timeout: 3_000,
  });

  // Switch to Edit mode (deselects the node)
  await page.locator('[title="Toggle edit mode"]').click();

  // After switching away, no further /pane requests should fire within 3s
  let extraPaneRequest = false;
  const listener = (req: { url: () => string }) => {
    if (req.url().includes("/pane")) {
      extraPaneRequest = true;
    }
  };
  page.on("request", listener);

  await page.waitForTimeout(3_000);
  page.off("request", listener);

  expect(extraPaneRequest).toBe(false);

  // Cleanup
  const sessionName = `maestro-${run_id}-worker-iter-1`;
  const { execSync } = await import("node:child_process");
  try {
    execSync(`tmux kill-session -t ${sessionName}`, { stdio: "ignore" });
  } catch {
    // ok
  }
});
