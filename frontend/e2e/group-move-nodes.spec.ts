import { test, expect, type Locator } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { openPipelineForEdit } from "./helpers";

// Layer 3b — #232 group-move-nodes fix.
// Box-/additive-selecting several nodes and dragging them as a group must move
// ALL selected nodes, not just the grabbed one. Pre-fix, onNodeDragStop wrote
// only the grabbed node's `view`; the re-derivation then reset every other
// selected node back to its stored position (the "snap-back" bug). This test is
// the discriminating check: after a two-node group drag, BOTH selected nodes
// keep the new delta and the un-selected node is untouched. Pre-fix, exactly
// one of the two moves and the other reverts.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-group-move-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);
const PROMPTS_DIR = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.prompts`);

// A valid post-refonte pipeline (ADR-0011): exactly one `start` and one `end`,
// every node named. Three work nodes (`alpha`, `beta`, `gamma`) are stacked
// vertically with clear space so a drag never overlaps a neighbour. We
// multi-select alpha+beta, grab beta, and drag; gamma is the negative control.
const SEED_YAML = `name: ${PIPELINE_NAME}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: right }
    view: { x: 0, y: 300 }
  - id: alpha
    name: alpha
    type: doc-only
    inputs:
      - { name: in, side: left }
    outputs:
      - { name: out, side: right }
    view: { x: 240, y: 100 }
  - id: beta
    name: beta
    type: doc-only
    inputs:
      - { name: in, side: left }
    outputs:
      - { name: out, side: right }
    view: { x: 240, y: 320 }
  - id: gamma
    name: gamma
    type: doc-only
    inputs:
      - { name: in, side: left }
    outputs:
      - { name: out, side: right }
    view: { x: 240, y: 540 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: left }
    view: { x: 520, y: 300 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: alpha, port: in }
  - source: { node: alpha, port: out }
    target: { node: beta, port: in }
  - source: { node: beta, port: out }
    target: { node: gamma, port: in }
  - source: { node: gamma, port: out }
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

// xyflow renders each node at `transform: translate(Xpx, Ypx)` in FLOW units
// (zoom is applied to the viewport container, not per-node), so the translate
// values mirror the node's stored `view` coords. Parse them off the inline
// style.
async function nodePos(node: Locator): Promise<{ x: number; y: number }> {
  return node.evaluate((el) => {
    const t = (el as HTMLElement).style.transform || getComputedStyle(el).transform;
    const m = t.match(/translate\(\s*(-?[\d.]+)px,\s*(-?[\d.]+)px\)/);
    if (m) return { x: parseFloat(m[1]), y: parseFloat(m[2]) };
    const mm = t.match(/matrix\(([^)]+)\)/);
    if (mm) {
      const p = mm[1].split(",").map((n) => parseFloat(n));
      return { x: p[4], y: p[5] };
    }
    return { x: 0, y: 0 };
  });
}

test("group drag moves every selected node, not just the grabbed one (#232)", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  await openPipelineForEdit(page, PIPELINE_NAME);

  const alpha = page.locator('.react-flow__node[data-id="alpha"]');
  const beta = page.locator('.react-flow__node[data-id="beta"]');
  const gamma = page.locator('.react-flow__node[data-id="gamma"]');
  await expect(alpha).toBeVisible({ timeout: 5_000 });
  await expect(beta).toBeVisible();
  await expect(gamma).toBeVisible();

  const alpha0 = await nodePos(alpha);
  const beta0 = await nodePos(beta);
  const gamma0 = await nodePos(gamma);

  // Multi-select alpha + beta: plain click alpha, then Control+click beta
  // (Control is the Linux/Windows multi-selection modifier).
  await alpha.click();
  await page.keyboard.down("Control");
  await beta.click();
  await page.keyboard.up("Control");

  // Drag the grabbed node (beta) with the proven incremental recipe — xyflow's
  // d3-drag ignores a single atomic move, so we step the mouse and let each
  // frame fire. Both selected nodes ride along.
  const box = await beta.boundingBox();
  if (!box) throw new Error("beta bounding box not found");
  const sx = box.x + box.width / 2;
  const sy = box.y + box.height / 2;
  const dx = 170;
  const dy = -60;

  await page.mouse.move(sx, sy);
  await page.mouse.down();
  const steps = 5;
  for (let i = 1; i <= steps; i++) {
    await page.mouse.move(sx + (dx * i) / steps, sy + (dy * i) / steps);
    await page.waitForTimeout(50);
  }
  await page.mouse.up();

  // Let onNodeDragStop → store write → re-derivation → setNodes settle.
  await page.waitForTimeout(300);

  const alpha1 = await nodePos(alpha);
  const beta1 = await nodePos(beta);
  const gamma1 = await nodePos(gamma);

  const dAlpha = { x: alpha1.x - alpha0.x, y: alpha1.y - alpha0.y };
  const dBeta = { x: beta1.x - beta0.x, y: beta1.y - beta0.y };
  const dGamma = { x: gamma1.x - gamma0.x, y: gamma1.y - gamma0.y };

  // Sanity: the grabbed node actually moved (rules out a no-op drag).
  expect(Math.abs(dBeta.x) + Math.abs(dBeta.y)).toBeGreaterThan(10);

  // DISCRIMINATING ASSERTION: alpha (the OTHER selected node) moved by the same
  // delta as beta. Pre-fix, alpha's view was never written, so the
  // re-derivation snapped it back (dAlpha ≈ 0 ≠ dBeta) and this fails.
  expect(dAlpha.x).toBeCloseTo(dBeta.x, 0);
  expect(dAlpha.y).toBeCloseTo(dBeta.y, 0);

  // Negative control: gamma was never selected, so it must not move.
  expect(Math.abs(dGamma.x)).toBeLessThan(2);
  expect(Math.abs(dGamma.y)).toBeLessThan(2);
});
