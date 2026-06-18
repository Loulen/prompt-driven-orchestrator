import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { openPipelineForEdit } from "./helpers";

// Layer 3b — #24 drag-no-teleport fix.
// Asserts that dragging a node in EditCanvas updates the node's CSS transform
// on intermediate frames (not only at drop), proving xyflow's controlled state
// is wired through onNodesChange.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-drag-teleport-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

// Post-refonte schema (ADR-0011): a valid pipeline needs exactly one `start`
// (zero inputs, one `user_prompt` output) and one `end` (one `result` input,
// zero outputs), plus a `name` on every node — otherwise the daemon rejects
// the YAML (400) and the edit canvas never opens. `dragger` is the node under
// test; start/end are placed off to the sides so the drag has clear space.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: right }
    view: { x: 0, y: 200 }
  - id: dragger
    name: dragger
    type: doc-only
    prompt_file: ${PIPELINE_NAME}.prompts/dragger.md
    inputs:
      - { name: in, side: left }
    outputs:
      - { name: out, side: right }
    view: { x: 200, y: 200 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: left }
    view: { x: 500, y: 200 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: dragger, port: in }
  - source: { node: dragger, port: out }
    target: { node: end, port: result }
`;

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
});

test("node CSS transform updates during drag, not only at drop (#24)", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Open the test pipeline into the edit canvas
  await openPipelineForEdit(page, PIPELINE_NAME);

  // Wait for the node to render inside the xyflow canvas
  const node = page.locator('.react-flow__node[data-id="dragger"]');
  await expect(node).toBeVisible({ timeout: 5_000 });

  // Capture the initial transform
  const initialTransform = await node.evaluate(
    (el) => getComputedStyle(el).transform || el.style.transform,
  );

  // Perform a slow drag via mouse events so intermediate frames fire
  const box = await node.boundingBox();
  if (!box) throw new Error("Node bounding box not found");

  const startX = box.x + box.width / 2;
  const startY = box.y + box.height / 2;
  const deltaX = 150;
  const deltaY = 80;

  await page.mouse.move(startX, startY);
  await page.mouse.down();

  // Move in small increments and collect intermediate transforms
  const intermediateTransforms: string[] = [];
  const steps = 5;
  for (let i = 1; i <= steps; i++) {
    await page.mouse.move(
      startX + (deltaX * i) / steps,
      startY + (deltaY * i) / steps,
    );
    // Small wait to let React re-render
    await page.waitForTimeout(50);
    const t = await node.evaluate(
      (el) => getComputedStyle(el).transform || el.style.transform,
    );
    intermediateTransforms.push(t);
  }

  await page.mouse.up();

  // The key assertion: at least one intermediate transform must differ from
  // the initial transform. Without onNodesChange wired up, xyflow would reset
  // the node back to its original position on every render, so all intermediate
  // transforms would equal the initial one (the "teleport" bug).
  const changed = intermediateTransforms.filter((t) => t !== initialTransform);
  expect(changed.length).toBeGreaterThan(0);

  // Also verify the final position differs from the start
  const finalTransform = await node.evaluate(
    (el) => getComputedStyle(el).transform || el.style.transform,
  );
  expect(finalTransform).not.toEqual(initialTransform);
});
