import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — Failed-node interactivity (#36).
// Verifies:
// 1. Selecting a failed node shows a red failure banner with the failure reason.
// 2. Mark complete button is visible for failed nodes.
// 3. Clicking Mark complete with missing outputs shows 409 sub-banner listing ports.
// 4. After creating output files, Mark complete succeeds and sub-banner clears.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-failed-node-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: worker
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: summary
      - name: report
    view: { x: 100, y: 100 }
edges: []
`;

let runId: string;

test.beforeAll(async () => {
  process.env.MAESTRO_TMUX_CMD_OVERRIDE = "exec sleep 300";
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  delete process.env.MAESTRO_TMUX_CMD_OVERRIDE;
  if (runId) {
    const { execSync } = await import("node:child_process");
    try {
      execSync(`tmux kill-session -t maestro-${runId}-worker-iter-1`, {
        stdio: "ignore",
      });
    } catch {
      // session may already be dead
    }
  }
});

async function createRunAndFailNode(baseURL: string, page: import("@playwright/test").Page) {
  const resp = await page.request.post(`${baseURL}/runs`, {
    data: {
      pipeline: PIPELINE_NAME,
      input: "e2e failed-node test",
    },
  });
  expect(resp.status()).toBe(201);
  const json = await resp.json();
  runId = json.run_id;

  // Wait for node to start running
  await page.waitForTimeout(1_000);

  // Fail the node via the API
  const failResp = await page.request.post(
    `${baseURL}/runs/${runId}/nodes/worker/fail`,
    { data: { reason: "tool call exited 1: command not found", iter: 1 } },
  );
  expect(failResp.status()).toBe(200);
}

test("failed node shows failure banner and Mark complete button", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  await createRunAndFailNode(baseURL!, page);

  // Navigate to the run
  await page.getByText(runId.slice(0, 8)).first().click({ timeout: 5_000 });

  // Click the worker node
  await page.waitForTimeout(500);
  const workerNode = page.getByText("worker", { exact: true }).first();
  await expect(workerNode).toBeVisible({ timeout: 3_000 });
  await workerNode.click();

  // Assert the red failure banner is visible with the failure reason
  await expect(
    page.getByText("Failed — tool call exited 1: command not found"),
  ).toBeVisible({ timeout: 5_000 });

  // Assert Mark complete button is visible
  await expect(page.getByText("Mark complete")).toBeVisible({ timeout: 3_000 });
});

test("Mark complete with missing outputs shows 409 sub-banner", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  if (!runId) {
    await createRunAndFailNode(baseURL!, page);
  }

  // Navigate to the run and select the failed node
  await page.getByText(runId.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  await page.getByText("worker", { exact: true }).first().click();
  await expect(page.getByText("Mark complete")).toBeVisible({ timeout: 5_000 });

  // Click Mark complete — no output files exist, expect 409 sub-banner
  await page.getByText("Mark complete").click();

  // Assert the sub-banner lists the missing output ports
  await expect(page.getByText("Missing outputs:")).toBeVisible({
    timeout: 5_000,
  });
  await expect(page.getByText("summary")).toBeVisible();
  await expect(page.getByText("report")).toBeVisible();
});

test("Mark complete succeeds after creating output files", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  if (!runId) {
    await createRunAndFailNode(baseURL!, page);
  }

  // Create the missing output files on disk
  const artifactsDir = path.join(
    WORKSPACE_ROOT,
    ".maestro",
    "runs",
    runId,
    "worktree",
    ".maestro",
    "artifacts",
  );
  const iterDir = path.join(artifactsDir, "worker", "iter-1");
  await fs.mkdir(iterDir, { recursive: true });
  await fs.writeFile(path.join(iterDir, "summary.md"), "# Summary\nDone.");
  await fs.writeFile(path.join(iterDir, "report.md"), "# Report\nAll good.");

  // Navigate to the run and select the failed node
  await page.getByText(runId.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  await page.getByText("worker", { exact: true }).first().click();
  await expect(page.getByText("Mark complete")).toBeVisible({ timeout: 5_000 });

  // Click Mark complete — should succeed now
  await page.getByText("Mark complete").click();

  // The sub-banner should not appear (or should clear)
  await page.waitForTimeout(1_000);
  await expect(page.getByText("Missing outputs:")).not.toBeVisible();

  // Node status should eventually transition to Completed
  await expect(page.getByText("Completed")).toBeVisible({ timeout: 5_000 });
});
