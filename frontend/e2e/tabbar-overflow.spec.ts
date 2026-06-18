import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — TabBar horizontal overflow scroll (#70).
// Verifies:
// 1. Save button stays visible when many tabs overflow the bar.
// 2. Tab list has horizontal scroll when overflowed.
// 3. Clicking a far-right tab auto-scrolls it into view.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const TAB_COUNT = 12;
const PREFIX = `e2e-tabbar-${process.pid}-${Date.now()}`;

function pipelineName(i: number): string {
  return `${PREFIX}-p${String(i).padStart(2, "0")}`;
}

// The strict parser (get_pipeline) requires every node to carry a `name` and the
// pipeline to have exactly one start (output `user_prompt`) and one end (input
// `result`) node. A pre-refonte single-bare-node seed 400s and the tab silently
// fails to open.
function seedYaml(name: string): string {
  return `name: ${name}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: alpha
    name: alpha
    type: doc-only
    inputs:
      - name: in
    outputs:
      - name: out
    view: { x: 100, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges: []
`;
}

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  for (let i = 0; i < TAB_COUNT; i++) {
    const name = pipelineName(i);
    await fs.writeFile(path.join(PIPELINE_DIR, `${name}.yaml`), seedYaml(name));
  }
});

test.afterAll(async () => {
  for (let i = 0; i < TAB_COUNT; i++) {
    const name = pipelineName(i);
    await fs.rm(path.join(PIPELINE_DIR, `${name}.yaml`), { force: true });
    await fs.rm(path.join(PIPELINE_DIR, `${name}.prompts`), { recursive: true, force: true });
  }
});

test("tabbar overflows horizontally, Save stays visible, active tab scrolls into view", async ({ page }) => {
  // Narrow viewport to force overflow with fewer tabs
  await page.setViewportSize({ width: 900, height: 600 });
  await page.goto("/");
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Post-refonte (#146): pipelines open into the edit canvas from the Library
  // tab — there is no pencil "Toggle edit mode" anymore.
  await page.getByRole("tab", { name: "Library" }).click();

  // Open all 12 pipelines as tabs by clicking their Library entries.
  for (let i = 0; i < TAB_COUNT; i++) {
    const name = pipelineName(i);
    const entry = page.getByText(name, { exact: true }).first();
    await expect(entry).toBeVisible({ timeout: 10_000 });
    await entry.click();
    // Each open adds a tab; wait for it before opening the next so the bar
    // actually overflows.
    await expect(page.getByTestId(`tab-title-${name}`)).toBeVisible({
      timeout: 5_000,
    });
  }

  // Save button must be visible and clickable regardless of tab count
  const saveBtn = page.getByTestId("save-button");
  await expect(saveBtn).toBeVisible();
  await expect(saveBtn).toBeDisabled(); // nothing dirty yet

  // The tab list container should have horizontal scroll (scrollWidth > clientWidth)
  const tabList = page.getByTestId("tab-list");
  const hasOverflow = await tabList.evaluate(
    (el) => el.scrollWidth > el.clientWidth,
  );
  expect(hasOverflow).toBe(true);

  // Click the first tab (far-left, potentially scrolled out of view after opening many)
  const firstTabName = pipelineName(0);
  const firstTab = page.getByTestId(`tab-title-${firstTabName}`);
  await firstTab.click();

  // The first tab should be scrolled into view (visible within the tab list viewport)
  await expect(firstTab).toBeVisible();
  const firstTabInView = await firstTab.evaluate((el) => {
    const container = el.closest('[data-testid="tab-list"]')!;
    const rect = el.getBoundingClientRect();
    const containerRect = container.getBoundingClientRect();
    return rect.left >= containerRect.left && rect.right <= containerRect.right;
  });
  expect(firstTabInView).toBe(true);

  // Now click the last tab (far-right)
  const lastTabName = pipelineName(TAB_COUNT - 1);
  const lastTab = page.getByTestId(`tab-title-${lastTabName}`);
  await lastTab.click();

  // The last tab should be scrolled into view
  const lastTabInView = await lastTab.evaluate((el) => {
    const container = el.closest('[data-testid="tab-list"]')!;
    const rect = el.getBoundingClientRect();
    const containerRect = container.getBoundingClientRect();
    return rect.left >= containerRect.left && rect.right <= containerRect.right;
  });
  expect(lastTabInView).toBe(true);

  // Save button must still be visible after all tab switching
  await expect(saveBtn).toBeVisible();
});
