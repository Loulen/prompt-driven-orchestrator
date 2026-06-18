import { type Page } from "@playwright/test";

/**
 * Open a repo/user pipeline into the edit canvas via the post-refonte
 * UnifiedLeftPanel: switch to the Library tab, then click the entry by name.
 *
 * Replaces the pre-canvas-refonte `[data-testid='edit-toggle']` flow, which no
 * longer exists — pipelines are opened from the Library tab now (#146).
 */
export async function openPipelineForEdit(page: Page, name: string): Promise<void> {
  await page.getByRole("tab", { name: "Library" }).click();
  await page.getByText(name).first().click({ timeout: 5_000 });
}
