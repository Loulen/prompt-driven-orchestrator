import { expect, type Locator } from "@playwright/test";

export async function expectNonZeroBBox(locator: Locator): Promise<void> {
  const box = await locator.boundingBox();
  expect(box).toBeTruthy();
  expect(box!.height).toBeGreaterThan(0);
  expect(box!.width).toBeGreaterThan(0);
}
