import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import { openPipelineForEdit, cleanupRuns } from "./helpers";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 5 — review-loop E2E (refs #52, ADR-0011).
//
// Post-canvas-refonte a loop is a named `loops:` region (NOT a `type: loop`
// node): a ≥2-member bounded region draws a translucent box whose header reads
// `↻ max N` while editing and `↻ i/N` during a run. This seeds
// start → implementer → reviewer → end with a reviewer→implementer back-edge
// (verdict ≠ PASS) and a reviewer→end exit (verdict = PASS), wrapped in a
// bounded region over [implementer, reviewer]. It verifies the region renders
// with its counter in edit mode, then drives one full lap via mark_node_done
// (impl done → reviewer FAIL) and asserts the region counter advances to 2/5.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-loop-review-${process.pid}-${Date.now()}`;
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
    view: { x: 0, y: 0 }
  - id: impl1
    name: implementer
    type: doc-only
    inputs:
      - { name: in, side: left }
    outputs:
      - { name: out, side: right }
    view: { x: 200, y: 160 }
  - id: reviewer
    name: reviewer
    type: doc-only
    inputs:
      - { name: in, side: left }
    outputs:
      - name: review
        side: right
        frontmatter:
          verdict:
            type: enum
            allowed: [PASS, FAIL]
    view: { x: 460, y: 160 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: left }
    view: { x: 720, y: 0 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: impl1, port: in }
  - source: { node: impl1, port: out }
    target: { node: reviewer, port: in }
  - source: { node: reviewer, port: review }
    target: { node: impl1, port: in }
    when:
      verdict: { neq: PASS }
  - source: { node: reviewer, port: review }
    target: { node: end, port: result }
    when:
      verdict: { eq: PASS }
loops:
  - id: review-loop
    kind: bounded
    members: [impl1, reviewer]
    max_iter: 5
`;

let runId: string;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await cleanupRuns(runId);
});

async function waitForNodeStatus(
  page: Page,
  baseURL: string,
  rid: string,
  nodeId: string,
  status: string,
  iter: number,
) {
  await expect(async () => {
    const resp = await page.request.get(`${baseURL}/runs/${rid}`);
    expect(resp.status()).toBe(200);
    const json = await resp.json();
    const node = json.nodes?.[nodeId];
    expect(node?.status).toBe(status);
    expect(node?.iter).toBe(iter);
  }).toPass({ timeout: 10_000 });
}

test("loop region renders in edit mode with max-iter counter", async ({
  page,
}) => {
  const consoleErrors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") consoleErrors.push(msg.text());
  });

  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Open the template into the edit canvas via the Library tab (post-refonte).
  await openPipelineForEdit(page, PIPELINE_NAME);

  // The bounded region over [implementer, reviewer] renders as a box; its header
  // shows the idle `↻ max 5` counter.
  const region = page.getByTestId("loop-region");
  await expect(region).toBeVisible({ timeout: 5_000 });
  const header = page.getByTestId("loop-region-header");
  await expect(header).toBeVisible({ timeout: 3_000 });
  await expect(header).toContainText("max 5");

  // Members render.
  await expect(page.getByText("implementer", { exact: true }).first()).toBeVisible({
    timeout: 3_000,
  });
  await expect(page.getByText("reviewer", { exact: true }).first()).toBeVisible({
    timeout: 3_000,
  });

  // Ignore transient resource 404s — cross-spec fixture churn (a sibling spec's
  // afterAll deleting its pipeline file while this page's list still references
  // it) surfaces as a network 404, which is harness noise, not a page error.
  expect(consoleErrors.filter((e) => !/Failed to load resource/.test(e))).toEqual([]);
});

test("loop run mode: region counter advances 1/5 → 2/5 over one lap", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Create a run via the API.
  const resp = await page.request.post(`${baseURL}/runs`, {
    data: { pipeline: PIPELINE_NAME, input: "loop review E2E test" },
  });
  expect(resp.status()).toBe(201);
  const json = await resp.json();
  runId = json.run_id;

  // implementer starts the first lap.
  await waitForNodeStatus(page, baseURL!, runId, "impl1", "running", 1);

  // Select the run; the canvas renders the loop region with the live counter.
  await page.getByText(runId.slice(0, 20)).first().click({ timeout: 5_000 });
  const region = page.getByTestId("loop-region");
  await expect(region).toBeVisible({ timeout: 5_000 });
  const header = page.getByTestId("loop-region-header");
  await expect(header).toContainText("1/5", { timeout: 5_000 });

  const artifactsBase = path.join(
    WORKSPACE_ROOT,
    ".pdo",
    "runs",
    runId,
    "worktree",
    ".pdo",
    "artifacts",
  );
  const seedOutput = async (nodeId: string, iter: number, port: string, body: string) => {
    const dir = path.join(artifactsBase, nodeId, `iter-${iter}`, port);
    await fs.mkdir(dir, { recursive: true });
    await fs.writeFile(path.join(dir, "output.md"), body);
  };
  const markDone = async (nodeId: string, iter: number) => {
    const r = await page.request.post(`${baseURL}/runs/${runId}/commands`, {
      data: { kind: "mark_node_done", node_id: nodeId, iter },
    });
    expect(r.status()).toBe(200);
  };

  // implementer lap 1 done → reviewer lap 1 runs.
  await seedOutput("impl1", 1, "out", "---\n---\n\nImplementation done.\n");
  await markDone("impl1", 1);
  await waitForNodeStatus(page, baseURL!, runId, "reviewer", "running", 1);

  // reviewer FAILs → the back-edge (verdict ≠ PASS) re-enters the loop at lap 2.
  await seedOutput("reviewer", 1, "review", "---\nverdict: FAIL\n---\n\nNeeds work.\n");
  await markDone("reviewer", 1);
  await waitForNodeStatus(page, baseURL!, runId, "impl1", "running", 2);

  // The region counter reflects the new lap.
  await expect(header).toContainText("2/5", { timeout: 5_000 });
});
