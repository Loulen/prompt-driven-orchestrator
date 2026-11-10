// The series colour of a harness, shared by every harness visualization (#638, #759).

import { cssColor } from "../../lib/cssColor";
import { currentMode, type Mode } from "../../lib/theme";

/** The harnesses with a pinned colour: `--harness-<name>` in `harness/harness.css`,
 *  per mode (#759: darkened on the light ground, hue kept). */
const PINNED_HARNESSES = new Set(["copilot", "claude"]);

/** Lightness of the generated series colour, per mode. */
const HARNESS_LIGHTNESS: Record<Mode, number> = { dark: 58, light: 34 };

/**
 * Stable series colour shared by every harness visualization.
 *
 * Reads the live mode off `<html>` rather than taking it as an argument: the
 * ten call sites sit in nested chart components, and the colour is presentation,
 * not data. `StatsCharts` subscribes to the theme so a switch re-renders them.
 */
export function harnessColor(name: string): string {
  const mode = currentMode();
  const normalized = name.trim().toLowerCase();
  if (PINNED_HARNESSES.has(normalized)) return cssColor(`--harness-${normalized}`);

  let hash = 2166136261;
  for (const char of normalized) {
    hash ^= char.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 16777619);
  }
  return `hsl(${(hash >>> 0) % 360} 64% ${HARNESS_LIGHTNESS[mode]}%)`;
}
