import { test, expect } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Layer 3b — resizable panels (testing pyramid per ADR 0004).
// Verifies that dragging a divider persists to localStorage and survives reload.
//
// Post canvas-refonte (#146/#57) there is a SINGLE unified layout: App always
// mounts one `ResizablePanelGroup` keyed to `pdo.layout.run`, whether the centre
// shows the empty placeholder or an open edit/run canvas. There is no longer a
// pencil "Toggle edit mode" with a separate `pdo.layout.edit` key — opening a
// pipeline into the canvas reuses the same panel group and the same layout key.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..");
const PIPELINE_NAME = `e2e-resizable-${process.pid}-${Date.now()}`;
const PIPELINE_DIR = path.join(WORKSPACE_ROOT, ".pdo", "pipelines");
const PIPELINE_PATH = path.join(PIPELINE_DIR, `${PIPELINE_NAME}.yaml`);

// A valid pipeline the strict parser (get_pipeline) accepts: every node needs a
// `name`, and there must be exactly one start (output `user_prompt`) and one end
// (input `result`) node. Pre-refonte seeds omitted these and 400'd silently.
const SEED_YAML = `name: ${PIPELINE_NAME}
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

test.beforeAll(async () => {
  await fs.mkdir(PIPELINE_DIR, { recursive: true });
  await fs.writeFile(PIPELINE_PATH, SEED_YAML);
});

test.afterAll(async () => {
  await fs.rm(PIPELINE_PATH, { force: true });
});

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("PDO").first()).toBeVisible();
  await expect(page.getByText("Daemon: connected")).toBeVisible({
    timeout: 10_000,
  });
});

test("drag divider persists layout to localStorage across reload", async ({
  page,
}) => {
  const handles = page.locator('[data-slot="resizable-handle"]');
  await expect(handles.first()).toBeVisible();

  const leftPanel = page.locator('[data-slot="resizable-panel"]').first();
  const initialWidth = await leftPanel.evaluate(
    (el) => el.getBoundingClientRect().width,
  );

  const handle = handles.first();
  const handleBox = await handle.boundingBox();
  expect(handleBox).toBeTruthy();

  // Drag the first handle 80px to the right to widen the left panel
  await page.mouse.move(
    handleBox!.x + handleBox!.width / 2,
    handleBox!.y + handleBox!.height / 2,
  );
  await page.mouse.down();
  await page.mouse.move(
    handleBox!.x + handleBox!.width / 2 + 80,
    handleBox!.y + handleBox!.height / 2,
    { steps: 5 },
  );
  await page.mouse.up();

  // Wait for the layout change to propagate to localStorage
  await page.waitForFunction(() => {
    const stored = localStorage.getItem("pdo.layout.run");
    return stored !== null;
  });

  const widthAfterDrag = await leftPanel.evaluate(
    (el) => el.getBoundingClientRect().width,
  );
  expect(widthAfterDrag).toBeGreaterThan(initialWidth);

  // Reload and verify size is preserved
  await page.reload();
  await expect(page.getByText("PDO").first()).toBeVisible();

  const leftPanelAfterReload = page
    .locator('[data-slot="resizable-panel"]')
    .first();
  const widthAfterReload = await leftPanelAfterReload.evaluate(
    (el) => el.getBoundingClientRect().width,
  );

  // Allow 2px tolerance for sub-pixel rounding
  expect(Math.abs(widthAfterReload - widthAfterDrag)).toBeLessThan(2);
});

test("layout is unified across the run placeholder and the edit canvas", async ({
  page,
}) => {
  const handles = page.locator('[data-slot="resizable-handle"]');
  await expect(handles.first()).toBeVisible();

  // Drag with the centre on the empty placeholder (no run/pipeline open yet).
  const handle = handles.first();
  const handleBox = await handle.boundingBox();
  expect(handleBox).toBeTruthy();
  await page.mouse.move(
    handleBox!.x + handleBox!.width / 2,
    handleBox!.y + handleBox!.height / 2,
  );
  await page.mouse.down();
  await page.mouse.move(
    handleBox!.x + handleBox!.width / 2 + 60,
    handleBox!.y + handleBox!.height / 2,
    { steps: 5 },
  );
  await page.mouse.up();

  await page.waitForFunction(() =>
    localStorage.getItem("pdo.layout.run") !== null,
  );
  const layoutBeforeEdit = await page.evaluate(() =>
    localStorage.getItem("pdo.layout.run"),
  );

  // Post-refonte there is no pencil "Toggle edit mode" and no separate
  // `pdo.layout.edit` key — the editor reuses the one unified layout.
  await expect(page.getByTitle("Toggle edit mode")).toHaveCount(0);
  expect(
    await page.evaluate(() => localStorage.getItem("pdo.layout.edit")),
  ).toBeNull();

  // Open a pipeline into the edit canvas; the same panel group stays mounted.
  await page.getByRole("tab", { name: "Library" }).click();
  const entry = page.getByText(PIPELINE_NAME).first();
  await expect(entry).toBeVisible({ timeout: 10_000 });
  await entry.click();
  await expect(page.getByTestId("tab-list")).toBeVisible({ timeout: 10_000 });

  // The layout the placeholder persisted is still in effect on the canvas.
  expect(
    await page.evaluate(() => localStorage.getItem("pdo.layout.run")),
  ).toBe(layoutBeforeEdit);

  // Drag again while the editor is open — it persists to the SAME key, and no
  // `pdo.layout.edit` key is ever created.
  const editHandle = page.locator('[data-slot="resizable-handle"]').first();
  const editBox = await editHandle.boundingBox();
  expect(editBox).toBeTruthy();
  await page.mouse.move(
    editBox!.x + editBox!.width / 2,
    editBox!.y + editBox!.height / 2,
  );
  await page.mouse.down();
  await page.mouse.move(
    editBox!.x + editBox!.width / 2 - 40,
    editBox!.y + editBox!.height / 2,
    { steps: 5 },
  );
  await page.mouse.up();

  await page.waitForFunction(
    (prev) => localStorage.getItem("pdo.layout.run") !== prev,
    layoutBeforeEdit,
  );

  expect(
    await page.evaluate(() => localStorage.getItem("pdo.layout.edit")),
  ).toBeNull();
  expect(
    await page.evaluate(() => localStorage.getItem("pdo.layout.run")),
  ).not.toBe(layoutBeforeEdit);
});
