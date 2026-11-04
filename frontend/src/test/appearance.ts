/**
 * Test helper: this browser's copy of the Apparence holds `elements` (element id →
 * shown), read at the next render; put back to the defaults when the test ends.
 */
import { onTestFinished } from "vitest";
import { resetAppearance } from "../hooks/useAppearance";
import { ELEMENTS_STORAGE_KEY } from "../lib/appearance";

export function withElements(elements: Record<string, boolean>): void {
  localStorage.setItem(ELEMENTS_STORAGE_KEY, JSON.stringify(elements));
  resetAppearance();
  onTestFinished(() => {
    localStorage.removeItem(ELEMENTS_STORAGE_KEY);
    resetAppearance();
  });
}
