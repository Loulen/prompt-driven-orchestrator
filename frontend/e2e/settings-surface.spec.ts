import { test, expect, type Page } from "@playwright/test";

// Layer 4 (e2e) per ADR 0004: the Settings surface in a real browser over a real daemon.
// The category rail and the section column, the search line, a setting changed then
// discarded, then saved with Ctrl+S and still there after a reload.

let reaperTtl = 300;

test.beforeEach(async ({ page, baseURL }) => {
  const settings = (await (await page.request.get(`${baseURL}/settings`)).json()) as {
    reaper_ttl_secs: { effective: number };
  };
  reaperTtl = settings.reaper_ttl_secs.effective;
});

// Shared daemon: put the value back as it was.
test.afterEach(async ({ page, baseURL }) => {
  await page.request.put(`${baseURL}/settings`, { data: { reaper_ttl_secs: reaperTtl } });
});

async function openSettings(page: Page) {
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await page.getByTestId("open-settings").click();
  await expect(page.getByTestId("setting-reaper-ttl")).toBeVisible();
}

test("rail, sections, search, discard, then a save that holds after a reload", async ({ page }) => {
  await openSettings(page);
  await expect(page.getByTestId("settings-category-general")).toHaveAttribute("aria-selected", "true");
  await expect(page.getByTestId("settings-header-category")).toHaveText("General");
  await expect(page.getByTestId("settings-subcolumn").getByRole("button")).toHaveCount(6);

  // The search line finds a setting in any category, and only it.
  await page.getByTestId("settings-search").fill("reaper");
  await expect(page.getByTestId("settings-search-count")).toHaveText("1");
  await expect(page.getByTestId("setting-reaper-ttl")).toBeVisible();
  await expect(page.getByTestId("setting-session-cap")).toHaveCount(0);
  // A switch of Settings › Appearance is found too, not only its section's title (CORE-232).
  await page.getByTestId("settings-search").fill("Cost");
  await expect(page.getByTestId("setting-row-runs.list.cost")).toBeVisible();
  // A tour is found by its name, and its row shows, not only the count (CORE-232).
  await page.getByTestId("settings-search").fill("full tour");
  await expect(page.getByTestId("tutorials-full")).toBeVisible();
  await page.getByTestId("settings-search").fill("");

  // A change shows the Save bar; Discard puts the value back.
  const ttl = page.getByTestId("setting-reaper-ttl");
  const before = await ttl.inputValue();
  const next = before === "4321" ? "4322" : "4321";
  await ttl.fill(next);
  await expect(page.getByTestId("settings-footer-status")).toHaveText("1 unsaved");
  await expect(page.getByTestId("settings-category-general-dirty")).toBeVisible();
  await page.getByTestId("settings-discard").click();
  await expect(ttl).toHaveValue(before);
  await expect(page.getByTestId("settings-footer")).toHaveCount(0);

  // Only the changed settings: none left, nothing to show.
  await page.getByTestId("settings-modified-only").click();
  await expect(page.getByText("No settings")).toBeVisible();
  await page.getByTestId("settings-modified-only").click();

  // Ctrl+S saves; the value holds after a reload.
  await ttl.fill(next);
  await page.keyboard.press("Control+s");
  await expect(page.getByTestId("settings-footer-status")).toHaveText("Saved");
  await page.reload();
  await page.getByTestId("open-settings").click();
  await expect(page.getByTestId("setting-reaper-ttl")).toHaveValue(next);
});
