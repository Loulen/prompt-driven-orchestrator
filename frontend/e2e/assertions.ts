import { expect, type Locator } from "@playwright/test";

/**
 * Assert the element occupies a real box on screen.
 *
 * **Retried, deliberately.** `boundingBox()` returns `null` — not a zero box —
 * for a DETACHED node, and the things we point this at replace their DOM node
 * asynchronously after they become visible: mermaid swaps the rendered `<svg>`
 * once layout settles, React Flow remounts on the first fit-view. So a
 * `toBeVisible()` that just passed can be followed by a `boundingBox()` that
 * samples the node one tick after it was replaced. That produced a reproducible
 * failure of `render-mermaid-artifact` under full-suite load (never in
 * isolation), on the sequence-diagram step that comes right after a modal
 * close/reopen.
 *
 * Polling makes the assertion about the **steady state** rather than a single
 * sampled instant. It weakens nothing: an element that never gets a real box
 * still fails, just after the timeout instead of on the first sample.
 */
export async function expectNonZeroBBox(locator: Locator): Promise<void> {
  await expect(async () => {
    const box = await locator.boundingBox();
    expect(box).toBeTruthy();
    expect(box!.height).toBeGreaterThan(0);
    expect(box!.width).toBeGreaterThan(0);
  }).toPass({ timeout: 10_000 });
}
