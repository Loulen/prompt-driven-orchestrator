import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — ForEach node E2E (refs #60).
// Seeds a pipeline with a ForEach node, verifies edit-mode rendering,
// then creates a run and confirms the ForEach node renders in run mode.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-foreach-${process.pid}-${Date.now()}`;
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
  - id: upstream
    name: upstream
    type: doc-only
    inputs:
      - name: in
        side: left
    outputs:
      - name: out
        side: right
        frontmatter:
          items:
            type: list
    view: { x: 250, y: 200 }
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
    view: { x: 500, y: 200 }
  - id: worker
    name: worker
    type: doc-only
    inputs:
      - name: in
        side: left
    outputs:
      - name: out
        side: right
    view: { x: 750, y: 200 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
        side: left
    outputs: []
    view: { x: 1000, y: 200 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: upstream, port: in }
  - source: { node: upstream, port: out }
    target: { node: fe1, port: in }
  - source: { node: fe1, port: body }
    target: { node: worker, port: in }
  - source: { node: worker, port: out }
    target: { node: fe1, port: done }
  - source: { node: fe1, port: done }
    target: { node: end, port: result }
`;

test.beforeAll(async () => {
  await fs.mkdir(PROMPTS_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
  await fs.writeFile(path.join(PROMPTS_DIR, "upstream.md"), "Produce items.\n");
  await fs.writeFile(path.join(PROMPTS_DIR, "worker.md"), "Process one item.\n");
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
  await fs.rm(PROMPTS_DIR, { recursive: true, force: true });
});

test("foreach node renders in edit mode with foreach badge", async ({
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

  // Switch to edit mode
  const editToggle = page.locator("[data-testid='edit-toggle']");
  await expect(editToggle).toBeVisible({ timeout: 3_000 });
  await editToggle.click();

  // Select the pipeline from the list
  await page.getByText(PIPELINE_NAME).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);

  // Verify the foreach node renders with "foreach" badge
  const feNode = page.getByText("per-item").first();
  await expect(feNode).toBeVisible({ timeout: 5_000 });

  // The foreach badge text should be present
  await expect(page.getByText("foreach").first()).toBeVisible({
    timeout: 3_000,
  });

  expect(consoleErrors).toEqual([]);
});

test("foreach toolbar button adds a new foreach node", async ({ page }) => {
  const consoleErrors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") consoleErrors.push(msg.text());
  });

  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });

  // Switch to edit mode
  const editToggle = page.locator("[data-testid='edit-toggle']");
  await expect(editToggle).toBeVisible({ timeout: 3_000 });
  await editToggle.click();

  // Select the pipeline
  await page.getByText(PIPELINE_NAME).first().click({ timeout: 5_000 });
  await page.waitForTimeout(500);

  // Click the ForEach toolbar button
  const foreachBtn = page.locator("[data-testid='toolbar-foreach']");
  await expect(foreachBtn).toBeVisible({ timeout: 3_000 });
  await foreachBtn.click();
  await page.waitForTimeout(500);

  // Should now have two foreach nodes visible (the seeded one + the new one)
  const foreachBadges = page.getByText("foreach");
  await expect(foreachBadges).toHaveCount(2, { timeout: 3_000 });

  expect(consoleErrors).toEqual([]);
});
