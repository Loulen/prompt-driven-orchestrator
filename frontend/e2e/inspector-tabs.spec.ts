import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { openPipelineForEdit } from "./helpers";

// Layer 3b — Inspector Run/Edit tabs (refs #68, refs #1).
// Verifies:
// 1. Run and Edit tabs render when a node is selected.
// 2. Default tab: active Run → Run; idle pipeline → Edit.
// 3. Tab selection is sticky across node selections, resets on reload.
// 4. Pending node shows placeholder and resolved inputs.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-inspector-tabs-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

// Two chained nodes: worker-a → worker-b.
// With TMUX_CMD_OVERRIDE='cat', worker-a stays running, worker-b stays pending.
// Post-refonte the parser requires exactly one start node (zero inputs, one
// output named `user_prompt`) and one end node (zero outputs, one input named
// `result`). The chain start → worker-a → worker-b → end keeps worker-a running
// (TMUX override `cat`) and worker-b pending.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
variables: {}
nodes:
  - id: start
    name: Start
    type: start
    inputs: []
    outputs:
      - name: user_prompt
    view: { x: 0, y: 100 }
  - id: worker-a
    name: Worker A
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 200, y: 100 }
  - id: worker-b
    name: Worker B
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 400, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    outputs: []
    view: { x: 600, y: 100 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker-a, port: in }
  - source: { node: worker-a, port: out }
    target: { node: worker-b, port: in }
  - source: { node: worker-b, port: out }
    target: { node: end, port: result }
`;

test.beforeAll(async () => {
  process.env.PDO_TMUX_CMD_OVERRIDE = 'exec sh -c "cat"';
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  delete process.env.PDO_TMUX_CMD_OVERRIDE;
});

test("active run: Run tab default, sticky across nodes, reload resets", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Create a run
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: { pipeline: PIPELINE_NAME, input: "tab test" },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();

  // Select the run in the left panel
  await page.getByText(run_id.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);

  // Click on worker-a node (running)
  await page
    .locator('.react-flow__node[data-id="worker-a"]')
    .click({ timeout: 5_000 });

  // Both tabs should be visible
  const runTab = page.getByTestId("inspector-tab-run");
  const editTab = page.getByTestId("inspector-tab-edit");
  await expect(runTab).toBeVisible({ timeout: 3_000 });
  await expect(editTab).toBeVisible();

  // Run tab should be active by default (active run)
  await expect(runTab).toHaveAttribute("data-active", "true");

  // Switch to Edit tab
  await editTab.click();
  await expect(editTab).toHaveAttribute("data-active", "true");

  // Select another node (worker-b) — Edit tab should stay active (sticky)
  await page
    .locator('.react-flow__node[data-id="worker-b"]')
    .click({ timeout: 3_000 });
  await expect(editTab).toHaveAttribute("data-active", "true");

  // Reload → select run → select running node → Run tab again
  await page.reload();
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });
  await page.getByText(run_id.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  await page
    .locator('.react-flow__node[data-id="worker-a"]')
    .click({ timeout: 5_000 });
  await expect(page.getByTestId("inspector-tab-run")).toHaveAttribute(
    "data-active",
    "true",
  );

  // Cleanup tmux sessions
  const { execSync } = await import("node:child_process");
  try {
    execSync(`tmux kill-session -t pdo-${run_id}-worker-a-iter-1`, {
      stdio: "ignore",
    });
  } catch {
    // ok
  }
  try {
    execSync(`tmux kill-session -t pdo-${run_id}-worker-b-iter-1`, {
      stdio: "ignore",
    });
  } catch {
    // ok
  }
  try {
    execSync(`tmux kill-session -t pdo-mgr-${run_id}`, {
      stdio: "ignore",
    });
  } catch {
    // ok
  }
});

test("idle pipeline: Edit tab default", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Open the pipeline template from the Library tab (post-refonte: no edit toggle).
  await openPipelineForEdit(page, PIPELINE_NAME);
  await page.waitForTimeout(500);

  // Click on a node
  await page
    .locator('.react-flow__node[data-id="worker-a"]')
    .click({ timeout: 5_000 });

  // Edit tab should be active by default (no run)
  await expect(page.getByTestId("inspector-tab-edit")).toHaveAttribute(
    "data-active",
    "true",
  );
});

test("pending node: Run tab shows placeholder and resolved inputs", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Create a run — worker-a runs, worker-b stays pending
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: { pipeline: PIPELINE_NAME, input: "pending test" },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();

  // Select the run
  await page.getByText(run_id.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);

  // Click on worker-b (pending)
  await page
    .locator('.react-flow__node[data-id="worker-b"]')
    .click({ timeout: 5_000 });

  // Run tab should be active (active run)
  await expect(page.getByTestId("inspector-tab-run")).toHaveAttribute(
    "data-active",
    "true",
  );

  // Should show pending placeholder
  await expect(page.getByTestId("pending-placeholder")).toBeVisible({
    timeout: 3_000,
  });
  await expect(page.getByTestId("pending-placeholder")).toContainText(
    "en attente",
  );

  // Cleanup
  const { execSync } = await import("node:child_process");
  try {
    execSync(`tmux kill-session -t pdo-${run_id}-worker-a-iter-1`, {
      stdio: "ignore",
    });
  } catch {
    // ok
  }
  try {
    execSync(`tmux kill-session -t pdo-mgr-${run_id}`, {
      stdio: "ignore",
    });
  } catch {
    // ok
  }
});
