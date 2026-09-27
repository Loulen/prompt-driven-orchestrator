// The series colour of a harness, shared by every harness visualization (#638, #759).

import { currentTheme, type ResolvedTheme } from "../../lib/theme";

const PINNED_HARNESS_COLORS: Record<ResolvedTheme, Record<string, string>> = {
  dark: { copilot: "#58a6ff", claude: "#f0883e" },
  // #759: the same two identities, darkened so they carry text and read as a
  // series against a light ground. Hue kept — the colour IS how a harness is
  // recognised across the stats surface.
  light: { copilot: "#0b5cc4", claude: "#a24409" },
};

/** Lightness of the generated series colour, per theme. */
const HARNESS_LIGHTNESS: Record<ResolvedTheme, number> = { dark: 58, light: 34 };

/**
 * Stable series colour shared by every harness visualization.
 *
 * Reads the live theme off `<html>` rather than taking it as an argument: the
 * ten call sites sit in nested chart components, and the colour is presentation,
 * not data. `StatsCharts` subscribes to the theme so a switch re-renders them.
 */
export function harnessColor(name: string): string {
  const theme = currentTheme();
  const normalized = name.trim().toLowerCase();
  const pinned = PINNED_HARNESS_COLORS[theme][normalized];
  if (pinned) return pinned;

  let hash = 2166136261;
  for (const char of normalized) {
    hash ^= char.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 16777619);
  }
  return `hsl(${(hash >>> 0) % 360} 64% ${HARNESS_LIGHTNESS[theme]}%)`;
}
