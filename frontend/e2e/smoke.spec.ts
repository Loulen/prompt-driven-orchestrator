import { test, expect } from "@playwright/test";

// Layer 3b smoke (testing pyramid per ADR 0004). Wires up the e2e harness:
// proves that Playwright can boot the real daemon, the bundle serves, the
// app mounts, and the WebSocket reaches "connected" status.

test("app mounts with header and reaches Daemon: connected", async ({ page }) => {
  await page.goto("/");

  await expect(page.getByText("Maestro").first()).toBeVisible();

  // The footer status pill flips to "Daemon: connected" once the WebSocket
  // hello round-trips. Allow up to 10s on a cold debug-build daemon.
  await expect(page.getByText("Daemon: connected")).toBeVisible({ timeout: 10_000 });
});
