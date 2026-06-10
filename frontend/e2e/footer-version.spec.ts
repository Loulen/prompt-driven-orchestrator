import { test, expect } from "@playwright/test";

// Layer 3b (testing pyramid per ADR 0004). The footer version must mirror the
// daemon's compiled version exposed on GET /sessions (#139) — the assertion
// that matters is endpoint↔footer equality, not any specific literal.

test("footer displays the daemon version from GET /sessions", async ({ page }) => {
  const resp = await page.request.get("/sessions");
  expect(resp.status()).toBe(200);
  const body = await resp.json();
  const version: string = body.version;
  expect(version).toMatch(/^\d+\.\d+\.\d+/);

  await page.goto("/");

  const footer = page.locator("footer");
  await expect(footer.getByText(`v${version}`, { exact: true })).toBeVisible({
    timeout: 10_000,
  });

  // Guard against the historical hardcoded literal resurfacing — unless the
  // daemon genuinely runs 0.1.0, in which case the equality check above
  // already covered it.
  if (version !== "0.1.0") {
    await expect(footer.getByText("v0.1.0", { exact: true })).toHaveCount(0);
  }
});
