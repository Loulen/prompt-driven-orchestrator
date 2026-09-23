import { test, expect } from "@playwright/test";
import { openPipelineForEdit } from "./helpers";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — Card ports and the rim drag-source (refs #66, #844).
//
// Post canvas-refonte / slim card (#149) and border wiring (#844): a node's
// INPUTS are emergent (an incoming arrow lands anywhere on the body, no input
// pill) — the only exception is the `merge` node's repeated `branches` input,
// which keeps a labelled pill. OUTPUTS are named on the EDGES that carry them,
// never on the card: there is no output dot and no output pill any more. A wire
// starts from any point of the card's border, through the four rim source strips
// (`rim-<side>`), and its preview grows on the wiring grid with no label chasing
// the cursor.
//
// This spec seeds one node of each kind that still parses (legacy `switch`/
// `loop` types migrate to generic agent nodes; `type: for-each` is hard-refused
// since ADR-0011 — its slot here is a plain non-isolated node with the same body/
// done port shape) and asserts: no output dot exists, the merge input pill is
// present, the four rim strips cover the border, and a drag from the rim draws
// the grid-snapped preview.

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
    type: agent
    isolated_worktree: false
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
    type: agent
    isolated_worktree: false
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
    type: agent
    isolated_worktree: true
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

test("no output dot is rendered; the merge input keeps its pill (#844)", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  await openPipelineForEdit(page, PIPELINE_NAME);
  await page.waitForTimeout(500);

  // The seed declares 10 output ports across the node types. NONE of them draws
  // anything on a card: an output is named on the edge that carries it (#845),
  // and the card's whole border is the drag-source instead (#844).
  await expect(page.locator('[data-testid^="port-output-"]')).toHaveCount(0);
  await expect(page.locator(".port-dot")).toHaveCount(0);
  await expect(
    page.locator('.react-flow__handle[data-handleid="plan"]'),
  ).toHaveCount(0);

  // Inputs are emergent (no input pill) — except the merge node's repeated
  // `branches` input, which keeps a labelled pill.
  await expect(page.getByTestId("port-input-branches")).toHaveCount(1);
  // No other input pills exist for the ordinary node types.
  await expect(page.getByTestId("port-input-task")).toHaveCount(0);
  await expect(page.getByTestId("port-input-in")).toHaveCount(0);
});

test("the whole card border is the drag-source (#844)", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  await openPipelineForEdit(page, PIPELINE_NAME);
  await page.waitForTimeout(500);

  // Four source strips per card that declares an output, one per side, each
  // stretched along its whole border — not a 6px square in a corner.
  const bottomRim = page.getByTestId("rim-source-bottom").first();
  await expect(bottomRim).toBeVisible({ timeout: 5_000 });
  const rimBox = await bottomRim.boundingBox();
  const card = page.locator(".react-flow__node").first();
  const cardBox = await card.boundingBox();
  if (!rimBox || !cardBox) throw new Error("card or rim not visible");
  expect(rimBox.width).toBeGreaterThan(cardBox.width * 0.9);

  for (const side of ["top", "bottom", "left", "right"]) {
    await expect(page.getByTestId(`rim-source-${side}`).first()).toHaveCount(1);
  }
});

test("dragging from the rim draws the grid-snapped preview, with no label (#844)", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  await openPipelineForEdit(page, PIPELINE_NAME);
  await page.waitForTimeout(500);

  const rim = page.getByTestId("rim-source-bottom").first();
  await expect(rim).toBeVisible({ timeout: 5_000 });
  const box = await rim.boundingBox();
  if (!box) throw new Error("rim not visible");

  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2, box.y + 140, { steps: 5 });

  await expect(page.getByTestId("drag-connection-line")).toBeVisible({
    timeout: 3_000,
  });
  // The snap indicator is the whole Shift feedback — filled on the grid, hollow
  // and dashed off it. Nothing textual follows the cursor any more.
  await expect(page.getByTestId("wiring-snap-indicator")).toHaveAttribute(
    "data-snapped",
    "true",
  );
  await page.keyboard.down("Shift");
  await page.mouse.move(box.x + box.width / 2 + 37, box.y + 173, { steps: 3 });
  await expect(page.getByTestId("wiring-snap-indicator")).toHaveAttribute(
    "data-snapped",
    "false",
  );
  await page.keyboard.up("Shift");

  await page.mouse.up();
});
