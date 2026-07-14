import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { cleanupRuns } from "./helpers";

// Layer 3b — Terminated-node default layout (#346).
//
// When a node's session has ended (completed / failed / stopped, or an archived
// run), opening the node in the Run inspector should default the terminal inset
// to *minimized* — a thin clickable bar — so the Outputs take the full height.
// The live terminal (`tmux-terminal`) is not mounted in that state, and the
// full-frame terminal (`terminal-fullsize`) is not shown either. Clicking the
// minimized bar (`term-restore`) restores the split view and mounts the
// terminal.
//
// `failed` is the reliably-reachable member of the terminated set in e2e (via
// the fail endpoint, as in failed-node.spec.ts); it exercises the exact same
// `nodeSessionEnded` → minimized code path as `completed` / `stopped`.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-terminated-minimized-${process.pid}-${Date.now()}`;
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
  await cleanupRuns(...createdRunIds);
});

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

test("a terminated node opens minimized with the Outputs on screen, and the bar restores the terminal", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // 1. Create the run and wait for the worker to spawn.
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: { pipeline: PIPELINE_NAME, input: "e2e terminated-node test" },
  });
  expect(resp.status()).toBe(201);
  const { run_id: runId } = await resp.json();
  createdRunIds.push(runId);
  await waitForWorkerStatus(page, baseURL!, runId, "running");

  // 2. Seed a `summary` output so the Outputs section has a clickable card.
  const summaryDir = path.join(
    WORKSPACE_ROOT,
    ".pdo",
    "runs",
    runId,
    "worktree",
    ".pdo",
    "artifacts",
    "worker",
    "iter-1",
    "summary",
  );
  await fs.mkdir(summaryDir, { recursive: true });
  await fs.writeFile(path.join(summaryDir, "output.md"), "# Summary\nDone.");

  // 3. Terminate the worker (failed ∈ the terminated set → minimized default).
  const failResp = await page.request.post(
    `${baseURL}/runs/${runId}/nodes/worker/fail`,
    { data: { reason: "e2e terminated-node", iter: 1 } },
  );
  expect(failResp.status()).toBe(200);
  await waitForWorkerStatus(page, baseURL!, runId, "failed");

  // 4. Select the run, then the worker node, then the Run tab — WITHOUT any
  //    terminal-reveal gesture (that is exactly what #346 removes the need for).
  await page
    .getByText(runId.slice(0, 20))
    .first()
    .click({ timeout: 5_000, position: { x: 5, y: 5 } });
  await page.waitForTimeout(500);
  const node = page.getByText("worker", { exact: true }).first();
  await expect(node).toBeVisible({ timeout: 5_000 });
  await node.click();

  const runTab = page.getByTestId("inspector-tab-run");
  if (await runTab.count()) {
    await runTab.click({ timeout: 5_000 });
  }
  const runPane = page.getByTestId("inspector-pane-run");

  // 5. Default layout (no click): terminal minimized, details + an Outputs card
  //    on screen, no full-frame terminal, no mounted live terminal.
  await expect(runPane.getByTestId("terminal-minimized")).toBeVisible({
    timeout: 5_000,
  });
  await expect(runPane.getByTestId("details-pane")).toBeVisible({
    timeout: 5_000,
  });
  await expect(runPane.locator("button.port-row").first()).toBeVisible({
    timeout: 10_000,
  });
  await expect(runPane.getByTestId("terminal-fullsize")).toHaveCount(0);
  await expect(runPane.getByTestId("tmux-terminal")).toHaveCount(0);

  // 6. Clicking the minimized bar restores the split view and mounts the
  //    terminal.
  await runPane.getByTestId("term-restore").click();
  await expect(runPane.getByTestId("tmux-terminal")).toBeVisible({
    timeout: 5_000,
  });
  await expect(runPane.getByTestId("terminal-minimized")).toHaveCount(0);
});
