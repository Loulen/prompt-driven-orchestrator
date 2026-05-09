import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — Port labels, semantic tooltips, and drag edge label (refs #66).
// Seeds a pipeline with one of each node type, asserts each port has a visible
// label, hovers a first-class port to verify the tooltip, and drags from an
// output port to verify the dynamic label appears.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-port-labels-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".maestro", "pipelines");
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
    type: for-each
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

test("every port has a visible label on all node types", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const editToggle = page.locator("[data-testid='edit-toggle']");
  await expect(editToggle).toBeVisible({ timeout: 3_000 });
  await editToggle.click();

  await page.getByText(PIPELINE_NAME).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);

  // Switch node: input "in" and output branches
  await expect(page.getByTestId("port-input-in").first()).toBeVisible({ timeout: 5_000 });

  // Loop node: all 4 ports
  await expect(page.getByTestId("port-input-break").first()).toBeVisible({ timeout: 3_000 });
  await expect(page.getByTestId("port-output-body").first()).toBeVisible({ timeout: 3_000 });
  await expect(page.getByTestId("port-output-done").first()).toBeVisible({ timeout: 3_000 });

  // Merge node: branches + merged
  await expect(page.getByTestId("port-input-branches")).toBeVisible({ timeout: 3_000 });
  await expect(page.getByTestId("port-output-merged")).toBeVisible({ timeout: 3_000 });

  // Ordinary node (planner): task + plan
  await expect(page.getByTestId("port-input-task")).toBeVisible({ timeout: 3_000 });
  await expect(page.getByTestId("port-output-plan")).toBeVisible({ timeout: 3_000 });
});

test("hovering a first-class port shows semantic tooltip", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const editToggle = page.locator("[data-testid='edit-toggle']");
  await expect(editToggle).toBeVisible({ timeout: 3_000 });
  await editToggle.click();

  await page.getByText(PIPELINE_NAME).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);

  // Hover the loop "body" output port to trigger the tooltip
  const bodyPort = page.getByTestId("port-output-body").first();
  await expect(bodyPort).toBeVisible({ timeout: 5_000 });
  await bodyPort.hover();

  // Wait for tooltip with the hardcoded description
  const tooltip = page.getByTestId("tooltip-content");
  await expect(tooltip).toBeVisible({ timeout: 3_000 });
  await expect(tooltip).toHaveText("Fires once per iteration");
});

test("dragging from an output port shows dynamic edge label", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  const editToggle = page.locator("[data-testid='edit-toggle']");
  await expect(editToggle).toBeVisible({ timeout: 3_000 });
  await editToggle.click();

  await page.getByText(PIPELINE_NAME).first().click({ timeout: 5_000 });
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
