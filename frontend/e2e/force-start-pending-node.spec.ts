import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { cleanupRuns } from "./helpers";

// Layer 3b — force-spawn a pending node via the UI Start button (#204).
//
// NOTE: this is NOT the unrelated `start-node.spec.ts` (that tests the canvas
// Start pseudo-node / StartInspector, #30). Here we test the Start *button* the
// NodeDetailPanel shows on a `pending` node, which force-spawns it out of
// dependency order through `POST /runs/{id}/nodes/{node}/start`.
//
// Chain: start → worker-a → worker-b → end. The daemon's e2e tmux stub keeps a
// node "running" (`sleep`), so worker-a runs and worker-b stays pending. We
// select worker-b, click its Start button, and assert worker-b transitions to
// running ahead of worker-a ever completing.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-force-start-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

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

let runId: string | undefined;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await cleanupRuns(runId);
  await fs.rm(PIPELINE_PATH, { force: true });
});

test("Start button force-spawns a pending downstream node", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Create a run — worker-a runs (held by the daemon's `sleep` stub),
  // worker-b stays pending behind it.
  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: { pipeline: PIPELINE_NAME, input: "force-start test" },
  });
  expect(resp.status()).toBe(201);
  ({ run_id: runId } = await resp.json());

  // worker-a should reach running so the pipeline is genuinely live.
  await expect(async () => {
    const r = await page.request.get(`${baseURL}/runs/${runId}`);
    expect(r.status()).toBe(200);
    expect((await r.json()).nodes?.["worker-a"]?.status).toBe("running");
  }).toPass({ timeout: 10_000 });

  // Select the run, then the pending downstream node.
  await page.getByText(runId!.slice(0, 8)).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);
  await page
    .locator('.react-flow__node[data-id="worker-b"]')
    .click({ timeout: 5_000 });

  // The Run tab is active for a live run; the pending node shows its placeholder
  // and — post-#204 — the Start button in the controls bar.
  await expect(page.getByTestId("inspector-tab-run")).toHaveAttribute(
    "data-active",
    "true",
    { timeout: 5_000 },
  );
  await expect(page.getByTestId("pending-placeholder")).toBeVisible({
    timeout: 5_000,
  });

  const startBtn = page.getByTestId("start-btn");
  await expect(startBtn).toBeVisible({ timeout: 5_000 });
  await startBtn.click();

  // worker-b should transition to running ahead of worker-a completing.
  await expect(async () => {
    const r = await page.request.get(`${baseURL}/runs/${runId}`);
    expect(r.status()).toBe(200);
    expect((await r.json()).nodes?.["worker-b"]?.status).toBe("running");
  }).toPass({ timeout: 10_000 });
});
