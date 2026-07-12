import { test, expect } from "@playwright/test";
import { openPipelineForEdit } from "./helpers";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — Port labels, hover label, and drag edge label (refs #66).
//
// Post canvas-refonte / slim card (#149, #170): a node's INPUTS are emergent
// (an incoming arrow lands anywhere on the body, no input pill) — the only
// exception is the `merge` node's repeated `branches` input, which keeps a
// labelled pill. OUTPUT ports render as plain filled dots that surface their
// name as a cursor-relative floating label (`.port-dot-lbl`) on hover, rather
// than an always-visible pill. Dragging an edge out of an output dot shows a
// dynamic `out/<port>` label on the connection line.
//
// This spec seeds one node of each kind that still parses (legacy `switch`/
// `loop` types migrate to generic agent nodes; `type: for-each` is hard-refused
// since ADR-0011 — its slot here is a plain doc-only node with the same body/
// done port shape) and asserts: every output port renders
// a dot, the merge input pill is present, hovering an output dot reveals its
// label, and dragging from an output dot shows the dynamic edge label.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-port-labels-${process.pid}-${Date.now()}`;
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
    view: { x: 0, y: 200 }
  - id: planner
    name: Planner
    type: doc-only
    inputs:
      - name: task
        side: left
        description: The task to plan
    outputs:
      - name: plan
        side: right
        description: The generated plan
    view: { x: 250, y: 100 }
  - id: sw1
    name: gate
    type: switch
    inputs:
      - name: in
        side: left
    outputs:
      - name: pass
        side: right
      - name: default
        side: right
    view: { x: 500, y: 100 }
  - id: loop1
    name: review-loop
    type: loop
    max_iter: 5
    inputs:
      - name: in
        side: left
      - name: break
        side: left
    outputs:
      - name: body
        side: right
      - name: done
        side: right
    view: { x: 500, y: 300 }
  - id: fe1
    name: per-item
    type: doc-only
    inputs:
      - name: in
        side: left
      - name: break
        side: left
    outputs:
      - name: body
        side: right
      - name: done
        side: right
    view: { x: 750, y: 100 }
  - id: mg1
    name: merger
    type: merge
    inputs:
      - name: branches
        side: left
        repeated: true
    outputs:
      - name: merged
        side: right
    view: { x: 750, y: 300 }
  - id: impl1
    name: implementer
    type: code-mutating
    inputs:
      - name: in
        side: left
    outputs:
      - name: out
        side: right
    view: { x: 1000, y: 200 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
    outputs: []
    view: { x: 1200, y: 200 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: planner, port: task }
  - source: { node: planner, port: plan }
    target: { node: sw1, port: in }
`;

test.beforeAll(async () => {
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "planner.md"), "Plan the task.\n");
  await fs.writeFile(path.join(PROMPTS_DIR, "implementer.md"), "Implement.\n");
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
});

test("every output port renders a dot and the merge input keeps its pill", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  await openPipelineForEdit(page, PIPELINE_NAME);
  await page.waitForTimeout(500);

  // Output ports render as dots (`port-output-<name>`). The seed declares 10
  // output ports across the node types: user_prompt, plan, pass, default,
  // body, done (loop), body, done (per-item doc-only), merged, out.
  const outputDots = page.locator('[data-testid^="port-output-"]');
  await expect(outputDots).toHaveCount(10, { timeout: 5_000 });

  // A few representative output dots are present in the DOM (the visible
  // element is the absolutely-positioned xyflow handle, not the wrapper div).
  await expect(page.getByTestId("port-output-plan")).toHaveCount(1);
  await expect(page.getByTestId("port-output-merged")).toHaveCount(1);
  // Their handle dots are rendered on the canvas.
  await expect(
    page.locator('.react-flow__handle[data-handleid="plan"][data-handlepos="right"]').first(),
  ).toBeVisible({ timeout: 3_000 });
  await expect(
    page.locator('.react-flow__handle[data-handleid="merged"][data-handlepos="right"]').first(),
  ).toBeVisible({ timeout: 3_000 });

  // Inputs are emergent (no input pill) — except the merge node's repeated
  // `branches` input, which keeps a labelled pill.
  await expect(page.getByTestId("port-input-branches")).toHaveCount(1);
  // No other input pills exist for the ordinary node types.
  await expect(page.getByTestId("port-input-task")).toHaveCount(0);
  await expect(page.getByTestId("port-input-in")).toHaveCount(0);
});

test("hovering an output port dot reveals its name as a floating label", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  await openPipelineForEdit(page, PIPELINE_NAME);
  await page.waitForTimeout(500);

  // The planner's `plan` output dot is the xyflow source handle.
  const planHandle = page
    .locator('.react-flow__handle[data-handleid="plan"][data-handlepos="right"]')
    .first();
  await expect(planHandle).toBeVisible({ timeout: 5_000 });

  const box = await planHandle.boundingBox();
  if (!box) throw new Error("plan handle not visible");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);

  // The cursor-relative floating label appears with the port name.
  const dotLabel = page.locator(".port-dot-lbl");
  await expect(dotLabel).toBeVisible({ timeout: 3_000 });
  await expect(dotLabel).toHaveText("plan");
});

test("dragging from an output port shows dynamic edge label", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  await openPipelineForEdit(page, PIPELINE_NAME);
  await page.waitForTimeout(500);

  // Find an output handle and drag from it
  // The planner's "plan" port handle has data-handleid="plan" and data-handlepos="right"
  const planHandle = page.locator(
    '.react-flow__handle[data-handleid="plan"][data-handlepos="right"]',
  );
  await expect(planHandle).toBeVisible({ timeout: 5_000 });

  const box = await planHandle.boundingBox();
  if (!box) throw new Error("plan handle not visible");

  // Start drag from the handle
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + 100, box.y + 50, { steps: 5 });

  // The drag connection line should be visible with the label
  const dragLine = page.getByTestId("drag-connection-line");
  await expect(dragLine).toBeVisible({ timeout: 3_000 });

  const dragLabel = page.getByTestId("drag-label-text");
  await expect(dragLabel).toBeVisible({ timeout: 3_000 });
  await expect(dragLabel).toHaveText(/out\/plan/);

  await page.mouse.up();
});
