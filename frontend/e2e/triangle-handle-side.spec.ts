import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 4 — Output port handle renders on the side named by its `side` field (#40).
//
// Post canvas-refonte (#149 / #170): output ports render as plain filled dots
// (`OutputPortDot`) whose xyflow Handle carries `data-handlepos="<side>"`. The
// old per-port SVG triangle polygon (points like "5,10 11,10 8,2") is gone, as
// are input pills — a node's inputs are emergent (no input handle). So this spec
// asserts the OUTPUT dot lands on the declared `side: top` (its handle has
// `data-handlepos="top"`), and that no left/right output handle is mislaid.
//
// The daemon refuses to load a pipeline without exactly one start + one end node
// (crates/pdo-daemon/src/pipeline.rs), so the seed wraps the checker between a
// start and an end. `POST /runs` is multipart/form-data post-refonte.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-triangle-side-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    inputs: []
    outputs:
      - name: user_prompt
    view: { x: 0, y: 100 }
  - id: checker
    name: checker
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/checker.md
    inputs:
      - name: task
        side: left
    outputs:
      - name: result
        side: top
    view: { x: 200, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
    outputs: []
    view: { x: 400, y: 100 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: checker, port: task }
  - source: { node: checker, port: result }
    target: { node: end, port: result }
`;

let runId: string;

test.beforeAll(async () => {
  process.env.PDO_TMUX_CMD_OVERRIDE =
    "exec sh -c \"sleep 300\"";
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "checker.md"), "Do the task.\n");
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
  delete process.env.PDO_TMUX_CMD_OVERRIDE;
  if (runId) {
    const { execSync } = await import("node:child_process");
    try {
      execSync(`tmux kill-session -t pdo-${runId}-checker-iter-1`, {
        stdio: "ignore",
      });
    } catch {
      // session may already be dead
    }
  }
});

test("output port with side:top renders its dot handle on the top edge", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const resp = await page.request.post(`${baseURL}/runs`, {
    multipart: {
      pipeline: PIPELINE_NAME,
      input: "triangle side test",
    },
  });
  expect(resp.status()).toBe(201);
  const json = await resp.json();
  runId = json.run_id;

  await page.getByText(runId.slice(0, 8)).first().click({ timeout: 5_000 });

  const reactFlow = page.locator(".react-flow");
  await expect(reactFlow).toBeVisible({ timeout: 5_000 });

  // Wait for the node to render
  await page.waitForTimeout(500);

  // The `result` output dot (#170) is the xyflow source Handle for the checker
  // node. Declared `side: top` → xyflow stamps it `data-handlepos="top"`.
  const resultHandle = page.locator(
    '.react-flow__handle[data-handleid="result"]',
  );
  await expect(resultHandle.first()).toBeVisible({ timeout: 5_000 });

  // The checker's output dot sits on the top edge.
  const checkerTopDot = page.locator(
    '.react-flow__handle.port-dot[data-handleid="result"][data-handlepos="top"]',
  );
  await expect(checkerTopDot).toBeVisible({ timeout: 5_000 });
  // It is rendered as a plain dot (the slim-card output dot), not a labelled
  // pill — the old per-port SVG triangle polygon was removed in the refonte.
  await expect(checkerTopDot.locator("polygon")).toHaveCount(0);

  // The checker output is NOT on the left or right edge (it honours side: top).
  await expect(
    page.locator(
      '.react-flow__handle.port-dot[data-handleid="result"][data-handlepos="left"]',
    ),
  ).toHaveCount(0);
  await expect(
    page.locator(
      '.react-flow__handle.port-dot[data-handleid="result"][data-handlepos="right"]',
    ),
  ).toHaveCount(0);
});
