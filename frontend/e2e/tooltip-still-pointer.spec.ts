import { test, expect } from "@playwright/test";

// The kit's tooltip waits for the pointer to move: a surface that opens under
// a still pointer shows no tooltip for the control that lands there (the
// engine only sends a pointerover then, no pointermove). A real hover shows it,
// and moving away hides it.

test("a modal opened under a still pointer shows no tooltip until the pointer moves", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await page.getByTestId("open-stats").click();
  const refresh = page.getByTestId("stats-refresh");
  await expect(refresh).toBeVisible();
  const box = (await refresh.boundingBox())!;
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.keyboard.press("Escape");
  await expect(refresh).toHaveCount(0);

  // Park the pointer where Refresh lands, then open Stats without moving it.
  await page.mouse.move(x, y);
  await page.evaluate(() => (document.querySelector('[data-testid="open-stats"]') as HTMLElement).click());
  await expect(refresh).toBeVisible();
  await page.waitForTimeout(800);
  await expect(page.getByRole("tooltip")).toHaveCount(0);

  // A real move on it shows the tooltip; leaving it, still, hides it.
  await page.mouse.move(x + 2, y);
  await expect(page.getByRole("tooltip")).toHaveText("Refresh");
  await page.mouse.move(x - 300, y + 200, { steps: 5 });
  await page.waitForTimeout(500);
  await expect(page.getByRole("tooltip")).toHaveCount(0);
});
