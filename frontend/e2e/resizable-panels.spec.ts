import { test, expect } from "@playwright/test";

// Layer 3b — resizable panels (testing pyramid per ADR 0004).
// Verifies that dragging a divider persists to localStorage and survives reload.

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Maestro").first()).toBeVisible();
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
    const stored = localStorage.getItem("maestro.layout.run");
    return stored !== null;
  });

  const widthAfterDrag = await leftPanel.evaluate(
    (el) => el.getBoundingClientRect().width,
  );
  expect(widthAfterDrag).toBeGreaterThan(initialWidth);

  // Reload and verify size is preserved
  await page.reload();
  await expect(page.getByText("Maestro").first()).toBeVisible();

  const leftPanelAfterReload = page
    .locator('[data-slot="resizable-panel"]')
    .first();
  const widthAfterReload = await leftPanelAfterReload.evaluate(
    (el) => el.getBoundingClientRect().width,
  );

  // Allow 2px tolerance for sub-pixel rounding
  expect(Math.abs(widthAfterReload - widthAfterDrag)).toBeLessThan(2);
});

test("run and edit modes have independent layouts", async ({ page }) => {
  const handles = page.locator('[data-slot="resizable-handle"]');
  await expect(handles.first()).toBeVisible();

  // Drag in run mode
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
    localStorage.getItem("maestro.layout.run") !== null,
  );
  const runLayout = await page.evaluate(() =>
    localStorage.getItem("maestro.layout.run"),
  );

  // Toggle to edit mode
  await page.getByTitle("Toggle edit mode").click();
  await expect(page.getByText("Edit")).toBeVisible();

  // Edit mode should NOT have the run layout in its key
  const editLayout = await page.evaluate(() =>
    localStorage.getItem("maestro.layout.edit"),
  );
  expect(editLayout).toBeNull();

  // Drag in edit mode
  const editHandles = page.locator('[data-slot="resizable-handle"]');
  await expect(editHandles.first()).toBeVisible();
  const editHandle = editHandles.first();
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

  await page.waitForFunction(() =>
    localStorage.getItem("maestro.layout.edit") !== null,
  );

  // Both keys should exist and be different
  const runAfter = await page.evaluate(() =>
    localStorage.getItem("maestro.layout.run"),
  );
  const editAfter = await page.evaluate(() =>
    localStorage.getItem("maestro.layout.edit"),
  );
  expect(runAfter).toBe(runLayout);
  expect(editAfter).not.toBe(runAfter);
});
