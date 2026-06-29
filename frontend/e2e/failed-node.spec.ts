import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { openRunNodeDetails, cleanupRuns } from "./helpers";

// Layer 3b — Failed-node interactivity (#36).
// Verifies:
// 1. A failed node shows a red failure banner with the reason + Mark complete.
// 2. Mark complete with missing outputs shows the 409 sub-banner listing ports.
// 3. Mark complete succeeds once the output files exist, and the node completes.
//
// Tests 2 and 3 operate on a *running* node, not a failed one: failing the only
// worker drives the whole run terminal (Failed), and the daemon then refuses
// `mark_node_done` with "resume the run first" (a different 409 with no `missing`
// list) rather than the missing-outputs 409 the sub-banner is built from. A
// running node in a live run is the state where Mark complete validates outputs,
// which is exactly what those two assertions exercise. Each test owns its run so
// they are independent of order and of each other's state.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-failed-node-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: bottom }
    view: { x: 100, y: 0 }
  - id: worker
    name: worker
    type: doc-only
    inputs:
      - { name: task, side: top }
    outputs:
      - { name: summary, side: bottom }
      - { name: report, side: right }
    view: { x: 100, y: 150 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: top }
    view: { x: 100, y: 300 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: summary }
    target: { node: end, port: result }
`;

const createdRunIds: string[] = [];

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  // Archive the runs so their stub sessions are reaped and they stop counting
  // toward the global session cap.
  await cleanupRuns(...createdRunIds);
});

// Poll the run until `worker` reaches `status`, so interactions never race the
// (separate-process) daemon spawning the node session.
async function waitForWorkerStatus(
  page: Page,
  baseURL: string,
  runId: string,
  status: string,
) {
  await expect(async () => {
    const resp = await page.request.get(`${baseURL}/runs/${runId}`);
    expect(resp.status()).toBe(200);
    const json = await resp.json();
    expect(json.nodes?.worker?.status).toBe(status);
  }).toPass({ timeout: 10_000 });
}

async function createRun(page: Page, baseURL: string): Promise<string> {
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: { pipeline: PIPELINE_NAME, input: "e2e failed-node test" },
  });
  expect(resp.status()).toBe(201);
  const { run_id } = await resp.json();
  createdRunIds.push(run_id);
  await waitForWorkerStatus(page, baseURL, run_id, "running");
  return run_id;
}

// Open the run, select the worker node, and reveal the Run inspector details
// pane where the failure banner / Mark-complete button live. The shared helper
// switches to the Run tab and collapses the terminal if the active run brought
// it up full-size (which would otherwise hide the details pane).
async function selectWorkerRunPane(page: Page, runId: string) {
  await openRunNodeDetails(page, runId, "worker");
}

test("failed node shows failure banner and Mark complete button", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const runId = await createRun(page, baseURL!);

  // Fail the worker via the API, then confirm it landed before driving the UI.
  const failResp = await page.request.post(
    `${baseURL}/runs/${runId}/nodes/worker/fail`,
    { data: { reason: "tool call exited 1: command not found", iter: 1 } },
  );
  expect(failResp.status()).toBe(200);
  await waitForWorkerStatus(page, baseURL!, runId, "failed");

  await selectWorkerRunPane(page, runId);

  // Red failure banner with the reason.
  await expect(
    page.getByText("Failed — tool call exited 1: command not found"),
  ).toBeVisible({ timeout: 5_000 });

  // Mark complete button is offered for a failed node.
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

  const runId = await createRun(page, baseURL!);

  await selectWorkerRunPane(page, runId);
  await expect(page.getByText("Mark complete")).toBeVisible({ timeout: 5_000 });

  // Click Mark complete — no output files exist, expect the 409 sub-banner.
  await page.getByText("Mark complete").click();

  // The banner is a single span "Missing outputs: summary, report" — assert on
  // it directly so the port-row labels/paths (also "summary"/"report") don't
  // trip strict mode.
  const missing = page.getByText(/^Missing outputs:/);
  await expect(missing).toBeVisible({ timeout: 5_000 });
  await expect(missing).toContainText("summary");
  await expect(missing).toContainText("report");
});

test("Mark complete succeeds after creating output files", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const runId = await createRun(page, baseURL!);

  // Create the missing output files on disk at <node>/iter-<N>/<port>/output.md.
  const iterDir = path.join(
    WORKSPACE_ROOT,
    ".pdo",
    "runs",
    runId,
    "worktree",
    ".pdo",
    "artifacts",
    "worker",
    "iter-1",
  );
  await fs.mkdir(path.join(iterDir, "summary"), { recursive: true });
  await fs.mkdir(path.join(iterDir, "report"), { recursive: true });
  await fs.writeFile(path.join(iterDir, "summary", "output.md"), "# Summary\nDone.");
  await fs.writeFile(path.join(iterDir, "report", "output.md"), "# Report\nAll good.");

  await selectWorkerRunPane(page, runId);
  await expect(page.getByText("Mark complete")).toBeVisible({ timeout: 5_000 });

  // Click Mark complete — should succeed now.
  await page.getByText("Mark complete").click();

  // The sub-banner should not appear.
  await page.waitForTimeout(1_000);
  await expect(page.getByText("Missing outputs:")).not.toBeVisible();

  // Node status transitions to Completed.
  await expect(page.getByText("Completed")).toBeVisible({ timeout: 5_000 });
});
